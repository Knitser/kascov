#!/usr/bin/env bash
# Deploy the kascov realtime worker to Cloud Run and point Firebase Hosting's
# /data/** at it. Prerequisite (one-time, manual):
#   gcloud billing projects link kascov-explorer --billing-account=<ACCOUNT_ID>
set -euo pipefail
cd "$(dirname "$0")/.."

PROJECT=kascov-explorer
REGION=europe-west4
SERVICE=kascov-worker
BUCKET=kascov-explorer-index

if [ "$(gcloud billing projects describe $PROJECT --format='value(billingEnabled)')" != "True" ]; then
  echo "billing is not enabled on $PROJECT — link a billing account first:" >&2
  echo "  gcloud billing projects link $PROJECT --billing-account=<ACCOUNT_ID>" >&2
  exit 1
fi

echo "==> enabling APIs"
gcloud services enable run.googleapis.com cloudbuild.googleapis.com artifactregistry.googleapis.com storage.googleapis.com monitoring.googleapis.com --project $PROJECT

echo "==> ensuring backup bucket gs://$BUCKET (+ versioning + lifecycle)"
gcloud storage buckets create "gs://$BUCKET" --project $PROJECT --location=$REGION 2>/dev/null || true
# Idempotent durability one-timers (previously tribal knowledge in a comment):
# object versioning so a bad 'latest' has history, and the lifecycle rule that
# prunes archive/ copies + old noncurrent versions (scripts/lifecycle.json).
gcloud storage buckets update "gs://$BUCKET" --versioning --project $PROJECT > /dev/null || true
gcloud storage buckets update "gs://$BUCKET" --lifecycle-file="$(dirname "$0")/lifecycle.json" --project $PROJECT > /dev/null || true

echo "==> ensuring Artifact Registry repo (kaniko layer cache + images)"
gcloud artifacts repositories create kascov \
  --repository-format=docker --location=$REGION --project $PROJECT 2>/dev/null || true

IMAGE="$REGION-docker.pkg.dev/$PROJECT/kascov/kascov-worker:latest"

# Kaniko caches every Docker layer in the registry (cloudbuild.yaml), so warm
# builds skip the cargo-chef dependency cook and the silverc language build:
# ~30 min cold → minutes warm.
echo "==> building $IMAGE via Cloud Build (kaniko layer cache)"
gcloud builds submit --config cloudbuild.yaml --project $PROJECT .

# --update-env-vars MERGES with the previous revision's env — never use
# --set-env-vars here: it REPLACES the whole env and once silently disarmed
# KASCOV_DEPLOY_KEY (the operator-set one-click-deploy key) on a redeploy.
echo "==> deploying $SERVICE to Cloud Run ($REGION)"
gcloud run deploy $SERVICE \
  --image "$IMAGE" \
  --project $PROJECT \
  --region $REGION \
  --allow-unauthenticated \
  --min-instances 1 \
  --max-instances 1 \
  --no-cpu-throttling \
  --memory 4Gi \
  --cpu 2 \
  --cpu-boost \
  --update-env-vars "^@^BACKUP_BUCKET=$BUCKET@NETWORKS=testnet-10,mainnet" \
  --port 8080

# Monitoring: two uptime checks + two log-based alert policies keyed on the
# KASCOV_RESTORE_*/KASCOV_BACKUP_* tokens that worker-entrypoint.sh logs.
# Everything is created WITHOUT a notification channel (channels are
# account-specific) — attaching one in the console is the manual step that
# arms the alerts; the pointer is printed below.
# ensure_uptime NAME PATH — `uptime create` is NOT idempotent (it happily
# minted one duplicate per deploy; 7 of each accumulated by Jul 18) — so
# look before creating, same discipline as ensure_log_alert below.
HOST=$(gcloud run services describe $SERVICE --project $PROJECT --region $REGION --format='value(status.url)' | sed 's|https://||')
ensure_uptime() {
  local name="$1" path="$2"
  if gcloud monitoring uptime list-configs --project "$PROJECT" \
      --filter="displayName='$name'" --format='value(name)' 2>/dev/null | grep -q .; then
    echo "    uptime check '$name' already exists"
    return 0
  fi
  gcloud monitoring uptime create "$name" \
    --resource-type=uptime-url \
    --resource-labels="host=$HOST,project_id=$PROJECT" \
    --path="$path" \
    --project $PROJECT 2>/dev/null \
    || echo "    (uptime create failed or CLI unsupported)"
}
echo "==> ensuring uptime check on /data/mainnet-live.json"
ensure_uptime kascov-live /data/mainnet-live.json

# "No backups happening" is not expressible as a log-based alert (log alerts
# fire on presence, not absence) — the /health uptime check is the liveness
# proxy until a metric-absence policy is worth the ceremony.
echo "==> ensuring uptime check on /health (GFE swallows /healthz on run.app)"
ensure_uptime kascov-healthz /health

# ensure_log_alert NAME REGEX — log-based alert policy, idempotent by
# displayName (`policies create` happily makes duplicates, so look first).
ensure_log_alert() {
  local name="$1" log_regex="$2"
  if gcloud alpha monitoring policies list --project "$PROJECT" \
      --filter="displayName='$name'" --format='value(name)' 2>/dev/null | grep -q .; then
    echo "    alert policy '$name' already exists"
    return 0
  fi
  local tmp
  tmp=$(mktemp)
  # conditionMatchedLog requires alertStrategy.notificationRateLimit.
  cat > "$tmp" <<EOF
{
  "displayName": "$name",
  "combiner": "OR",
  "enabled": true,
  "conditions": [
    {
      "displayName": "$name log match",
      "conditionMatchedLog": {
        "filter": "resource.type=\\"cloud_run_revision\\" AND resource.labels.service_name=\\"$SERVICE\\" AND textPayload=~\\"$log_regex\\""
      }
    }
  ],
  "alertStrategy": {
    "notificationRateLimit": { "period": "3600s" },
    "autoClose": "604800s"
  }
}
EOF
  gcloud alpha monitoring policies create --policy-from-file="$tmp" --project "$PROJECT" > /dev/null \
    && echo "    created alert policy '$name'" \
    || echo "    (could not create '$name' — create it in the console with log filter: textPayload=~\"$log_regex\")"
  rm -f "$tmp"
}

echo "==> ensuring log-based alert policies (restore/backup trouble)"
ensure_log_alert kascov-restore-trouble "KASCOV_RESTORE_FRESH|KASCOV_RESTORE_FAIL"
ensure_log_alert kascov-backup-fail "KASCOV_BACKUP_FAIL"

echo ""
echo "    MANUAL STEP (user-gated): the alert policies and uptime checks have NO"
echo "    notification channel — nothing emails/pages you until one is attached."
echo "    In the console (Monitoring > Alerting > pick policy > edit), attach your"
echo "    channel to:"
echo "      - kascov-restore-trouble   (restore fell back to fresh, or failed)"
echo "      - kascov-backup-fail       (a periodic/final backup failed)"
echo "      - kascov-live / kascov-healthz uptime checks"
echo "    NOTE: the /healthz uptime check will FAIL until the next worker deploy"
echo "    ships the real /healthz endpoint — expected, safe to ignore until then."
echo ""

echo "==> granting the service account access to the backup bucket"
SA=$(gcloud run services describe $SERVICE --project $PROJECT --region $REGION --format='value(spec.template.spec.serviceAccountName)')
[ -n "$SA" ] || SA="$(gcloud projects describe $PROJECT --format='value(projectNumber)')-compute@developer.gserviceaccount.com"
gcloud storage buckets add-iam-policy-binding "gs://$BUCKET" --member="serviceAccount:$SA" --role=roles/storage.objectAdmin --project $PROJECT > /dev/null

echo "==> pointing Firebase Hosting /data/** at the worker (activates firebase-worker.json)"
cp firebase-worker.json firebase.json
# Hosting ships whatever is in web/ ON DISK — refuse a dirty tree so agent
# work-in-progress or local experiments never leak to production. (This once
# shipped an in-flight galaxy.js; deploy hosting from a clean checkout, or
# FORCE_DIRTY_HOSTING=1 to override deliberately.)
if ! git diff --quiet -- web/ || [ -n "$(git ls-files --others --exclude-standard web/ | grep -v '^web/data/')" ]; then
  if [ "${FORCE_DIRTY_HOSTING:-}" != "1" ]; then
    echo "!!  web/ has uncommitted changes — SKIPPING hosting deploy (worker was deployed)." >&2
    echo "    commit first, or rerun with FORCE_DIRTY_HOSTING=1" >&2
  else
    firebase deploy --only hosting --non-interactive
  fi
else
  firebase deploy --only hosting --non-interactive
fi

URL=$(gcloud run services describe $SERVICE --project $PROJECT --region $REGION --format='value(status.url)')
echo "==> done. worker: $URL"
echo "    site data now live via https://kascov.io/data/testnet-10.json"
