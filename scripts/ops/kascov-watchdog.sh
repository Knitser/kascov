#!/usr/bin/env bash
# Restart the worker when its own health endpoint says it is wedged.
#
# /healthz already reports 503 when a follower stops making progress, but until
# now nothing read it: the Cloud Monitoring policies that used to watch it died
# with the Cloud Run service. Caddy serves the SPA straight from disk, so a dead
# worker still returns 200 on the homepage and a naive uptime check sees nothing
# wrong. A wedge that goes unnoticed costs history the network has already
# pruned (a previous one ran 49 hours), so this is the cheapest real guard.
set -uo pipefail
STATE=/run/kascov-watchdog.fails
LAST=/run/kascov-watchdog.lastrestart
RESTARTS=/run/kascov-watchdog.restarts
ALERT=/usr/local/bin/kascov-alert.sh
NEED=3          # consecutive bad checks (timer is 60s) before acting
COOLDOWN=900    # never restart more than once per 15 min

code=$(curl -s -m 10 -o /dev/null -w '%{http_code}' http://127.0.0.1:8080/healthz 2>/dev/null || echo 000)
if [ "$code" = "200" ]; then
  echo 0 > "$STATE"; echo "healthz=200 ok"; exit 0
fi

fails=$(( $(cat "$STATE" 2>/dev/null || echo 0) + 1 ))
echo "$fails" > "$STATE"
echo "healthz=$code unhealthy ($fails/$NEED)"
[ "$fails" -lt "$NEED" ] && exit 0

now=$(date +%s); last=$(cat "$LAST" 2>/dev/null || echo 0)
if [ $(( now - last )) -lt "$COOLDOWN" ]; then
  echo "in cooldown ($(( now - last ))s since last restart), not restarting"; exit 0
fi
echo "$now" > "$LAST"; echo 0 > "$STATE"
restarts=$(( $(cat "$RESTARTS" 2>/dev/null || echo 0) + 1 )); echo "$restarts" > "$RESTARTS"
echo "RESTARTING kascov-worker after $fails consecutive unhealthy checks (last code $code)"
# self-healing must escalate: an unheard restart is how the 49-hour wedge
# happened. /run resets at boot, so the counter reads "since boot".
[ -x "$ALERT" ] && "$ALERT" "watchdog is restarting kascov-worker on $(hostname): healthz=$code, $fails consecutive failed checks, restart #$restarts since boot" || true
systemctl restart kascov-worker.service
