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

backup_loop() {
  [ -n "$BACKUP_BUCKET" ] || return 0
  while true; do
    sleep "$BACKUP_EVERY"
    token=$(gcs_token) || continue
    for n in $(echo "$NETWORKS" | tr ',' ' '); do
      [ -f "$DB_DIR/$n.db" ] || continue
      if kascov --network "$n" --db "$DB_DIR/$n.db" backup --out "/tmp/$n.bak" 2>/dev/null; then
        curl -sf -X POST -H "Authorization: Bearer $token" \
          -H "Content-Type: application/octet-stream" \
          --data-binary "@/tmp/$n.bak" \
          "https://storage.googleapis.com/upload/storage/v1/b/$BACKUP_BUCKET/o?uploadType=media&name=$n.db" \
          > /dev/null && echo "[entrypoint] backed up $n.db"
        rm -f "/tmp/$n.bak"
      fi
    done
  done
}

restore
backup_loop &
exec kascov serve --listen "0.0.0.0:${PORT:-8080}" --networks "$NETWORKS" --db-dir "$DB_DIR"
