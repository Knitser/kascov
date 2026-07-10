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
  --update-env-vars "^@^BACKUP_BUCKET=$BUCKET@NETWORKS=testnet-10,mainnet" \
  --port 8080

# Uptime check on the small always-cheap health path; alerting needs a
# notification channel, which is account-specific — print the pointer instead
# of guessing.
echo "==> ensuring uptime check on /data/mainnet-live.json"
HOST=$(gcloud run services describe $SERVICE --project $PROJECT --region $REGION --format='value(status.url)' | sed 's|https://||')
gcloud monitoring uptime create kascov-live \
  --resource-type=uptime-url \
  --resource-labels="host=$HOST,project_id=$PROJECT" \
  --path=/data/mainnet-live.json \
  --project $PROJECT 2>/dev/null \
  || echo "    (uptime check exists or CLI unsupported — attach an alert policy + email channel in the console)"

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
