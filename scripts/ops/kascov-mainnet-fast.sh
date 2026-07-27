#!/usr/bin/env bash
# Off-site the mainnet DB on a 5 minute cadence instead of 24 hours.
#
# mainnet.db is the SOLE archive of mainnet covenant history: public nodes prune
# at ~30h, so anything lost between the last off-site copy and a failure is gone
# for good. A 24h RPO on the one irreplaceable file was the largest avoidable
# exposure on this box.
#
# Gated on a LOGICAL fingerprint (event count + tip DAA), not on the file bytes.
# The follower rewrites its sync cursor every block, so the bytes differ on
# essentially every run: hashing the snapshot would ship ~157MB every 5 minutes
# (~45GB/day) to record nothing new. Covenant history is what must survive, so
# the archive only moves when covenant history actually moved, and an idle cycle
# costs one indexed query and no VACUUM and no egress.
set -uo pipefail
BIN=/home/kascov/kascov/target/release/kascov
DB=/home/kascov/kascov-db/mainnet.db
WORK=/home/kascov/kascov-backups/mainnet-fast.db
FPFILE=/home/kascov/kascov-backups/.mainnet-fast.fingerprint
[ -f "$DB" ] || { echo "no mainnet db"; exit 0; }

# read-only probe; never blocks the follower's writer
fp=$(python3 - "$DB" <<'PY' 2>/dev/null
import sqlite3, sys
c = sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True)
n, d = c.execute("SELECT COUNT(*), COALESCE(MAX(accepting_daa),0) FROM covenant_events").fetchone()
print(f"{n}:{d}")
PY
)
[ -z "$fp" ] && { echo "FAIL fingerprint probe"; exit 1; }
old=$(cat "$FPFILE" 2>/dev/null || echo none)
if [ "$fp" = "$old" ]; then echo "no new covenant history ($fp), skipping"; exit 0; fi

rm -f "$WORK" "$WORK-wal" "$WORK-shm"
"$BIN" --network mainnet --db "$DB" backup --out "$WORK" 2>/dev/null || { echo "FAIL backup"; exit 1; }

# never ship a corrupt snapshot over a good one
python3 -c "import sqlite3,sys;c=sqlite3.connect('file:$WORK?mode=ro',uri=True);sys.exit(0 if c.execute('PRAGMA quick_check').fetchone()[0]=='ok' else 1)" 2>/dev/null \
  || { echo "FAIL quick_check"; rm -f "$WORK"; exit 1; }

if python3 /home/kascov/kascov-offsite-one.py "$WORK" mainnet; then
  echo "$fp" > "$FPFILE"; echo "UPLOADED mainnet $(stat -c%s "$WORK") bytes at $fp"
else
  echo "FAIL upload (fingerprint not recorded, next run retries)"; exit 1
fi
