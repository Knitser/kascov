#!/bin/sh
# Cloud Run entrypoint: restore index DBs from GCS, run the worker, and
# back the DBs up periodically so restarts don't lose covenant history.
set -u
DB_DIR="${DB_DIR:-/data}"
NETWORKS="${NETWORKS:-testnet-10,mainnet}"
BACKUP_BUCKET="${BACKUP_BUCKET:-}"
BACKUP_EVERY="${BACKUP_EVERY:-300}"
mkdir -p "$DB_DIR"

gcs_token() {
  curl -s -H "Metadata-Flavor: Google" \
    "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token" \
    | jq -r .access_token
}

restore() {
  [ -n "$BACKUP_BUCKET" ] || return 0
  token=$(gcs_token) || return 0
  for n in $(echo "$NETWORKS" | tr ',' ' '); do
    if curl -sf -H "Authorization: Bearer $token" \
      "https://storage.googleapis.com/storage/v1/b/$BACKUP_BUCKET/o/$n.db?alt=media" \
      -o "$DB_DIR/$n.db"; then
      echo "[entrypoint] restored $n.db from gs://$BACKUP_BUCKET"
    else
      rm -f "$DB_DIR/$n.db"
      echo "[entrypoint] no backup for $n yet, starting fresh"
    fi
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

backup_loop() {
  [ -n "$BACKUP_BUCKET" ] || return 0
  # One-time operator setup for real durability (run once, NOT here):
  #   gcloud storage buckets update gs://$BACKUP_BUCKET --versioning
  #   gcloud storage buckets update gs://$BACKUP_BUCKET \
  #     --lifecycle-file=lifecycle.json   # e.g. keep 30 noncurrent versions
  #   # and an uptime alert on the health path:
  #   gcloud monitoring uptime create kascov-live --resource-type=uptime-url \
  #     --host=<cloud-run-host> --path=/data/mainnet-live.json   # + a policy/channel
  while true; do
    sleep "$BACKUP_EVERY"
    token=$(gcs_token) || continue
    stamp=$(date -u +%Y%m%d-%H%M%S)
    for n in $(echo "$NETWORKS" | tr ',' ' '); do
      [ -f "$DB_DIR/$n.db" ] || continue
      kascov --network "$n" --db "$DB_DIR/$n.db" backup --out "/tmp/$n.bak" 2>/dev/null || continue
      if ! valid_sqlite "/tmp/$n.bak"; then
        echo "[entrypoint] WARNING: $n backup failed validation — NOT uploading (preserving last good backup)"
        rm -f "/tmp/$n.bak"
        continue
      fi
      # Timestamped archival copy first (history survives even without bucket
      # versioning), then advance the stable 'latest' that restore reads.
      gcs_put "/tmp/$n.bak" "$token" "archive/$n-$stamp.db" || true
      if gcs_put "/tmp/$n.bak" "$token" "$n.db"; then
        echo "[entrypoint] backed up $n.db (+ archive/$n-$stamp.db)"
      fi
      rm -f "/tmp/$n.bak"
    done
  done
}

restore
backup_loop &
exec kascov serve --listen "0.0.0.0:${PORT:-8080}" --networks "$NETWORKS" --db-dir "$DB_DIR"
