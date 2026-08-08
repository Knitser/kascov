#!/usr/bin/env bash
# Post one message to the ops Discord channel.
#
# Every escalation path on the box funnels through here: unit OnFailure= hooks,
# the watchdog's restart branch, the restore-proof, and deploy.sh. The webhook
# URL is a capability (anyone holding it can post into the channel), so it
# lives outside the repo in /home/kascov/.kascov-alert.env (chmod 600), the
# same pattern as .deploy-key. Delivery is best-effort BY DESIGN: an alert
# failure must never turn a self-healing action or a deploy into a second
# incident, so every exit path here is 0.
#
# Install at /usr/local/bin/kascov-alert.sh so both root units (watchdog) and
# kascov-user units can reach it.
set -uo pipefail
ENVFILE=/home/kascov/.kascov-alert.env

MSG="${1:-}"
[ -n "$MSG" ] || { echo "kascov-alert: no message argument, nothing sent"; exit 0; }

if [ -z "${KASCOV_ALERT_WEBHOOK:-}" ] && [ -r "$ENVFILE" ]; then
  . "$ENVFILE"
fi
if [ -z "${KASCOV_ALERT_WEBHOOK:-}" ]; then
  echo "WARNING: KASCOV_ALERT_WEBHOOK unset and $ENVFILE absent -- ALERT NOT DELIVERED: $MSG" >&2
  exit 0
fi

# Discord caps content at 2000 chars; JSON-encode via python so the message may
# safely contain quotes, newlines, and anything a failing unit prints.
payload=$(python3 -c 'import json,sys; print(json.dumps({"content": sys.argv[1][:1900]}))' "$MSG" 2>/dev/null) \
  || { echo "WARNING: payload encode failed -- ALERT NOT DELIVERED: $MSG" >&2; exit 0; }

code=$(curl -s -m 10 -o /dev/null -w '%{http_code}' \
  -H 'Content-Type: application/json' -d "$payload" "$KASCOV_ALERT_WEBHOOK" 2>/dev/null || echo 000)
case "$code" in
  2*) echo "alert delivered ($code): $MSG" ;;
  *)  echo "WARNING: webhook answered $code -- ALERT NOT DELIVERED: $MSG" >&2 ;;
esac
exit 0
