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
gcloud services enable run.googleapis.com cloudbuild.googleapis.com artifactregistry.googleapis.com storage.googleapis.com --project $PROJECT

echo "==> ensuring backup bucket gs://$BUCKET"
gcloud storage buckets create "gs://$BUCKET" --project $PROJECT --location=$REGION 2>/dev/null || true

# the image now also builds + bundles the silverc compiler (a second, heavy
# build stage cloning kaspanet/silverscript), so give Cloud Build more room.
echo "==> extending Cloud Build timeout for the silverc bundling stage"
gcloud config set builds/timeout 3600 --installation 2>/dev/null || gcloud config set builds/timeout 3600

echo "==> deploying $SERVICE to Cloud Run ($REGION) — with silverc bundled, ~30 min"
gcloud run deploy $SERVICE \
  --source . \
  --project $PROJECT \
  --region $REGION \
  --allow-unauthenticated \
  --min-instances 1 \
  --max-instances 1 \
  --no-cpu-throttling \
  --memory 4Gi \
  --cpu 2 \
  --set-env-vars "^@^BACKUP_BUCKET=$BUCKET@NETWORKS=testnet-10,mainnet" \
  --port 8080

echo "==> granting the service account access to the backup bucket"
SA=$(gcloud run services describe $SERVICE --project $PROJECT --region $REGION --format='value(spec.template.spec.serviceAccountName)')
[ -n "$SA" ] || SA="$(gcloud projects describe $PROJECT --format='value(projectNumber)')-compute@developer.gserviceaccount.com"
gcloud storage buckets add-iam-policy-binding "gs://$BUCKET" --member="serviceAccount:$SA" --role=roles/storage.objectAdmin --project $PROJECT > /dev/null

echo "==> pointing Firebase Hosting /data/** at the worker (activates firebase-worker.json)"
cp firebase-worker.json firebase.json
firebase deploy --only hosting --non-interactive

URL=$(gcloud run services describe $SERVICE --project $PROJECT --region $REGION --format='value(status.url)')
echo "==> done. worker: $URL"
echo "    site data now live via https://kascov.io/data/testnet-10.json"
