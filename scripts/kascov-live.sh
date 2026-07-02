#!/usr/bin/env bash
# kascov live worker: keeps the index following the chain and republishes
# the dashboard snapshot every few minutes.
# Usage: scripts/kascov-live.sh [refresh-seconds]   (default 180)
set -u
cd "$(dirname "$0")/.."
REFRESH="${1:-180}"
BIN=./target/release/kascov
[ -x "$BIN" ] || BIN=./target/debug/kascov

"$BIN" --network testnet-10 sync --follow >> /tmp/kascov-live-sync.log 2>&1 &
SYNC_PID=$!
trap 'kill $SYNC_PID 2>/dev/null' EXIT

echo "[kascov-live] sync following (pid $SYNC_PID), republishing every ${REFRESH}s"
while true; do
  sleep "$REFRESH"
  "$BIN" --network testnet-10 export >/dev/null 2>&1
  "$BIN" --network mainnet export --out web/data/mainnet.json >/dev/null 2>&1
  if firebase deploy --only hosting --non-interactive >/dev/null 2>&1; then
    echo "[kascov-live] $(date '+%H:%M:%S') republished"
  else
    echo "[kascov-live] $(date '+%H:%M:%S') deploy failed, will retry next cycle"
  fi
done
