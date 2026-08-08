#!/usr/bin/env bash
# The ONE canonical VPS deploy: repo -> binary -> served web -> verified live.
#
# On 2026-08-07 a hand-rolled rebuild script quietly built a stale checkout and
# restarted the worker on an old commit; nothing failed, nothing alerted, and
# the "deployed" change was not live. This script exists so that can never
# recur: it always hard-resets to origin/main, bakes the commit hash into the
# binary, and refuses to call the deploy done until /healthz reports that exact
# hash back from the running process.
#
# Run as the kascov user. Install OUTSIDE the checkout (e.g. /home/kascov/
# deploy.sh): it hard-resets /home/kascov/kascov mid-run, and a script that
# rewrites itself while executing fails in silent, confusing ways.
set -euo pipefail
REPO=/home/kascov/kascov
WEBDST=/mnt/c/kascov/web
HEALTH=http://127.0.0.1:8080/healthz
ALERT=/usr/local/bin/kascov-alert.sh

alert() { if [ -x "$ALERT" ]; then "$ALERT" "$1" || true; fi; }

. "$HOME/.cargo/env"
cd "$REPO"
git fetch origin
git reset --hard origin/main
KASCOV_GIT_HASH=$(git rev-parse --short HEAD)
# exported for the BUILD: the worker embeds it at compile time and reports it
# as the "build" field of /healthz; the service's runtime env is untouched
export KASCOV_GIT_HASH
echo "deploying $KASCOV_GIT_HASH"

cargo build --release -p kascov

# Publish static web files per file, copy-only, on content difference. NEVER
# delete in the target: the served root holds files the repo does not
# (ops/traffic/traffic.json is written there every minute by the traffic
# timer), so any mirroring delete destroys live data.
copied=0
while IFS= read -r f; do
  dst="$WEBDST/${f#web/}"
  if [ ! -f "$dst" ] || ! cmp -s "$f" "$dst"; then
    mkdir -p "$(dirname "$dst")"
    cp -f "$f" "$dst"
    copied=$((copied+1))
    echo "published ${f#web/}"
  fi
done < <(git ls-files web/ | grep -vE '^web/(kascov\.html|kascov/)')
# ^ the /kascov trade page is UNLAUNCHED: it stays in the repo but is not
#   published. Because this loop re-copies anything missing from the target,
#   removing it server-side alone would resurrect it on the next deploy; this
#   filter is the single switch. Delete the grep to launch.
echo "published $copied changed file(s) to $WEBDST"

sudo systemctl restart kascov-worker.service

# the deploy is only real when the running worker reports the hash just built;
# anything else is the 2026-08-07 failure mode again
deadline=$(( $(date +%s) + 60 ))
build=""
while [ "$(date +%s)" -lt "$deadline" ]; do
  build=$(curl -s -m 5 "$HEALTH" \
    | python3 -c 'import json,sys;print(json.load(sys.stdin).get("build",""))' 2>/dev/null || true)
  if [ "$build" = "$KASCOV_GIT_HASH" ]; then break; fi
  sleep 2
done
if [ "$build" != "$KASCOV_GIT_HASH" ]; then
  echo "FAIL: /healthz build='$build' != deployed $KASCOV_GIT_HASH after 60s"
  alert "deploy FAILED on $(hostname): built $KASCOV_GIT_HASH but /healthz reports build='$build' after 60s"
  exit 1
fi
echo "DEPLOYED $KASCOV_GIT_HASH verified live"
