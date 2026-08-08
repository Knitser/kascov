#!/usr/bin/env bash
# Prove, weekly, that the off-site mainnet backup actually restores.
#
# The box's own GCS key (/home/kascov/.gcs-backup-key.json, objectCreator) is
# write-only BY DESIGN: a compromised box can add copies but cannot read or
# delete history. That same property means the box cannot verify its own
# uploads, so this job authenticates with a separate READ-scoped key whose
# path is $KASCOV_RESTORE_KEY_JSON. A backup nobody has ever restored is a
# hope, not a backup, and a restore-proof that silently skips is worse than
# none: a missing key alerts and exits 1 instead of going green.
set -euo pipefail
LIVE=/home/kascov/kascov-db/mainnet.db
SCRATCH_DIR=/home/kascov/kascov-restore-verify
SCRATCH=$SCRATCH_DIR/restored-mainnet.db
ALERT=/usr/local/bin/kascov-alert.sh
BUCKET=kascov-explorer-index
PREFIX=vps-backups/mainnet-
ALERTED=0

alert() { if [ -x "$ALERT" ]; then "$ALERT" "$1" || true; fi; }
fail() {
  echo "FAIL: $*"
  alert "restore-proof FAILED on $(hostname): $*"
  ALERTED=1
  exit 1
}
# every failure must reach the webhook, including ones that never call fail()
trap 'rc=$?; if [ "$rc" -ne 0 ] && [ "$ALERTED" -eq 0 ]; then alert "restore-proof crashed on $(hostname) (exit $rc); see journalctl -u kascov-restore-verify"; fi' EXIT

[ -n "${KASCOV_RESTORE_KEY_JSON:-}" ] \
  || fail "KASCOV_RESTORE_KEY_JSON is unset; cannot read the bucket (the box's own key is write-only by design)"
[ -r "$KASCOV_RESTORE_KEY_JSON" ] \
  || fail "read key $KASCOV_RESTORE_KEY_JSON missing or unreadable"
[ -f "$LIVE" ] \
  || fail "live DB $LIVE missing; nothing to compare the restore against"

mkdir -p "$SCRATCH_DIR"
# stale -wal/-shm sidecars next to a swapped-in SQLite file corrupt it (see RUNBOOK)
rm -f "$SCRATCH" "$SCRATCH-wal" "$SCRATCH-shm"

name=$(python3 - "$KASCOV_RESTORE_KEY_JSON" "$SCRATCH" <<PYEOF
import sys
from google.cloud import storage
key, dest = sys.argv[1], sys.argv[2]
client = storage.Client.from_service_account_json(key)
blobs = [b for b in client.bucket("$BUCKET").list_blobs(prefix="$PREFIX") if b.name.endswith(".db")]
if not blobs:
    sys.exit("no $PREFIX*.db objects in gs://$BUCKET")
# newest by upload time, not by name: the prefix matches both timestamped and
# fixed-name objects, and lexicographic order ranks a fixed name above any date
newest = max(blobs, key=lambda b: b.updated)
newest.download_to_filename(dest)
print(newest.name)
PYEOF
) || fail "download from gs://$BUCKET/$PREFIX* failed"
echo "downloaded $name ($(stat -c%s "$SCRATCH") bytes)"

python3 -c "import sqlite3,sys;c=sqlite3.connect('file:$SCRATCH?mode=ro',uri=True);sys.exit(0 if c.execute('PRAGMA quick_check').fetchone()[0]=='ok' else 1)" \
  || fail "PRAGMA quick_check not ok on restored $name"

restored=$(python3 -c "import sqlite3;print(sqlite3.connect('file:$SCRATCH?mode=ro',uri=True).execute('SELECT COUNT(*) FROM covenant_events').fetchone()[0])") \
  || fail "covenant row count failed on restored $name"
live=$(python3 -c "import sqlite3;print(sqlite3.connect('file:$LIVE?mode=ro',uri=True).execute('SELECT COUNT(*) FROM covenant_events').fetchone()[0])") \
  || fail "covenant row count failed on live DB"

[ "$restored" -gt 0 ] || fail "restored $name holds zero covenant rows"
# shrink tolerance 0: the archive only grows, so the live DB must never hold
# fewer rows than any backup ever taken from it
if [ "$restored" -gt "$live" ]; then
  fail "restored $name has $restored covenant rows, live has $live; live history has been lost"
fi

echo "OK $name restores: quick_check ok, covenant rows restored=$restored live=$live (lag $(( live - restored )))"
rm -f "$SCRATCH" "$SCRATCH-wal" "$SCRATCH-shm"
