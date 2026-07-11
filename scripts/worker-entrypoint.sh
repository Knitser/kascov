#!/bin/sh
# Cloud Run entrypoint: restore index DBs from GCS, run the worker, and
# back the DBs up periodically so restarts don't lose covenant history.
set -u
DB_DIR="${DB_DIR:-/data}"
NETWORKS="${NETWORKS:-testnet-10,mainnet}"
BACKUP_BUCKET="${BACKUP_BUCKET:-}"
BACKUP_EVERY="${BACKUP_EVERY:-300}"
mkdir -p "$DB_DIR"

# Prints an access token or returns 1. curl|jq quirk: when curl dies, jq sees
# empty input and still exits 0 — so failure is detected on the emptiness of
# the output, never on the pipeline status.
gcs_token() {
  t=$(curl -s -H "Metadata-Flavor: Google" \
    "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token" \
    | jq -r '.access_token // empty')
  [ -n "$t" ] || return 1
  echo "$t"
}

# restore_one NET — download gs://$BACKUP_BUCKET/NET.db into $DB_DIR, fail CLOSED.
# The bucket is the only archive of covenant history (nodes prune), so a failed
# restore must never be papered over with an empty index:
#   404 from GCS  -> object genuinely absent; fresh start ONLY if KASCOV_FRESH_OK=1
#   anything else -> transient (token fetch, network, 5xx, truncated body);
#                    retried with 5s/15s/45s backoff, then exit 1 so Cloud Run
#                    restarts this instance instead of it serving amnesia.
restore_one() {
  n="$1"
  tmp="$DB_DIR/$n.db.restore"
  for delay in 0 5 15 45; do
    if [ "$delay" -gt 0 ]; then
      echo "[entrypoint] restore $n: retrying in ${delay}s"
      sleep "$delay"
    fi
    if ! token=$(gcs_token); then
      echo "[entrypoint] restore $n: metadata token fetch failed (transient)"
      continue
    fi
    code=$(curl -s -H "Authorization: Bearer $token" \
      "https://storage.googleapis.com/storage/v1/b/$BACKUP_BUCKET/o/$n.db?alt=media" \
      -o "$tmp" -w '%{http_code}') || code=000
    case "$code" in
      200)
        if valid_sqlite "$tmp"; then
          mv "$tmp" "$DB_DIR/$n.db"
          echo "[entrypoint] KASCOV_RESTORE_OK net=$n bytes=$(wc -c < "$DB_DIR/$n.db" | tr -d ' ')"
          return 0
        fi
        echo "[entrypoint] restore $n: downloaded object is not a valid SQLite DB (truncated?)"
        ;;
      404)
        rm -f "$tmp"
        if [ "${KASCOV_FRESH_OK:-}" = "1" ]; then
          rm -f "$DB_DIR/$n.db"
          echo "[entrypoint] KASCOV_RESTORE_FRESH net=$n — no backup object at gs://$BACKUP_BUCKET/$n.db, starting fresh (KASCOV_FRESH_OK=1)"
          return 0
        fi
        echo "[entrypoint] KASCOV_RESTORE_FAIL net=$n reason=absent — gs://$BACKUP_BUCKET/$n.db does not exist"
        echo "[entrypoint] REFUSING to start with an empty index: this bucket is the only archive of covenant history."
        echo "[entrypoint] If a fresh index really is intended, redeploy with KASCOV_FRESH_OK=1."
        exit 1
        ;;
      *)
        echo "[entrypoint] restore $n: transient failure (http=$code)"
        ;;
    esac
    rm -f "$tmp"
  done
  echo "[entrypoint] KASCOV_RESTORE_FAIL net=$n reason=transient — all 4 attempts failed; exiting so Cloud Run restarts this instance"
  exit 1
}

restore() {
  if [ -z "$BACKUP_BUCKET" ]; then
    echo "[entrypoint] BACKUP_BUCKET not set — skipping restore (local/dev mode)"
    return 0
  fi
  for n in $(echo "$NETWORKS" | tr ',' ' '); do
    restore_one "$n"
  done
}

# A .bak is only safe to upload if it opens as a real, non-empty SQLite file —
# otherwise a corrupt/truncated DB would clobber the only backup. This checks the
# 16-byte "SQLite format 3\0" magic + a sane size without needing the sqlite3 CLI.
valid_sqlite() {
  f="$1"
  [ -s "$f" ] || return 1
  [ "$(wc -c < "$f")" -ge 512 ] || return 1
  head -c 16 "$f" | grep -q "SQLite format 3" || return 1
}

# gcs_put FILE OBJECT — upload FILE to gs://$BACKUP_BUCKET/OBJECT.
gcs_put() {
  curl -sf -X POST -H "Authorization: Bearer $2" \
    -H "Content-Type: application/octet-stream" \
    --data-binary "@$1" \
    "https://storage.googleapis.com/upload/storage/v1/b/$BACKUP_BUCKET/o?uploadType=media&name=$3" \
    > /dev/null
}

# The KASCOV_BACKUP_OK / KASCOV_BACKUP_FAIL / KASCOV_RESTORE_* log tokens feed
# the log-based alert policies created by scripts/deploy-worker.sh — keep them
# stable, one per line.
backup_loop() {
  [ -n "$BACKUP_BUCKET" ] || return 0
  # Bucket versioning + lifecycle + the uptime checks/alert policies are
  # applied idempotently by scripts/deploy-worker.sh (see scripts/lifecycle.json).
  while true; do
    sleep "$BACKUP_EVERY"
    if ! token=$(gcs_token); then
      echo "[entrypoint] KASCOV_BACKUP_FAIL reason=token — metadata token fetch failed, retrying next cycle"
      continue
    fi
    stamp=$(date -u +%Y%m%d-%H%M%S)
    for n in $(echo "$NETWORKS" | tr ',' ' '); do
      [ -f "$DB_DIR/$n.db" ] || continue
      if ! kascov --network "$n" --db "$DB_DIR/$n.db" backup --out "/tmp/$n.bak" 2>/dev/null; then
        echo "[entrypoint] KASCOV_BACKUP_FAIL net=$n reason=snapshot — kascov backup exited nonzero"
        continue
      fi
      if ! valid_sqlite "/tmp/$n.bak"; then
        echo "[entrypoint] KASCOV_BACKUP_FAIL net=$n reason=validation — NOT uploading (preserving last good backup)"
        rm -f "/tmp/$n.bak"
        continue
      fi
      bytes=$(wc -c < "/tmp/$n.bak" | tr -d ' ')
      # Timestamped archival copy first (history survives even without bucket
      # versioning), then advance the stable 'latest' that restore reads.
      gcs_put "/tmp/$n.bak" "$token" "archive/$n-$stamp.db" \
        || echo "[entrypoint] KASCOV_BACKUP_FAIL net=$n reason=archive-upload — latest will still advance"
      if gcs_put "/tmp/$n.bak" "$token" "$n.db"; then
        echo "[entrypoint] KASCOV_BACKUP_OK net=$n bytes=$bytes"
      else
        echo "[entrypoint] KASCOV_BACKUP_FAIL net=$n reason=upload"
      fi
      rm -f "/tmp/$n.bak"
    done
  done
}

# One last backup on shutdown. Cloud Run SIGTERMs on every redeploy/recycle
# (grace ~10s); without this, up to BACKUP_EVERY seconds of state die with the
# instance — and verified_sources / webhook_subscriptions / reorg_log are NOT
# re-derivable from the chain. Timestamped log lines make a truncated grace
# window visible after the fact.
final_backup() {
  [ -n "$BACKUP_BUCKET" ] || exit 0
  echo "[entrypoint] $(date -u +%H:%M:%S) SIGTERM — final backup starting"
  if ! token=$(gcs_token); then
    echo "[entrypoint] KASCOV_BACKUP_FAIL reason=token phase=final"
    exit 0
  fi
  for n in $(echo "$NETWORKS" | tr ',' ' '); do
    [ -f "$DB_DIR/$n.db" ] || continue
    if ! kascov --network "$n" --db "$DB_DIR/$n.db" backup --out "/tmp/$n.final.bak" 2>/dev/null; then
      echo "[entrypoint] KASCOV_BACKUP_FAIL net=$n reason=snapshot phase=final"
      continue
    fi
    if ! valid_sqlite "/tmp/$n.final.bak"; then
      echo "[entrypoint] KASCOV_BACKUP_FAIL net=$n reason=validation phase=final — NOT uploading"
    elif gcs_put "/tmp/$n.final.bak" "$token" "$n.db"; then
      echo "[entrypoint] KASCOV_BACKUP_OK net=$n bytes=$(wc -c < "/tmp/$n.final.bak" | tr -d ' ') phase=final"
    else
      echo "[entrypoint] KASCOV_BACKUP_FAIL net=$n reason=upload phase=final"
    fi
    rm -f "/tmp/$n.final.bak"
  done
  echo "[entrypoint] $(date -u +%H:%M:%S) final backup done"
  exit 0
}

restore
backup_loop &
LOOP_PID=$!
kascov serve --listen "0.0.0.0:${PORT:-8080}" --networks "$NETWORKS" --db-dir "$DB_DIR" &
SERVE_PID=$!
trap 'kill "$LOOP_PID" 2>/dev/null; kill "$SERVE_PID" 2>/dev/null; wait "$SERVE_PID" 2>/dev/null; final_backup' TERM INT
wait "$SERVE_PID"
