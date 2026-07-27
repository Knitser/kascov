#!/usr/bin/env python3
"""Upload one already-made snapshot off-site. Split from kascov-offsite.py so the
fast mainnet path can ship a fresh file it just built, rather than re-uploading
whatever {net}-latest.db the 6-hourly job last wrote."""
import os, sys, datetime
from google.cloud import storage

KEY = "/home/kascov/.gcs-backup-key.json"
BUCKET = "kascov-explorer-index"
src, net = sys.argv[1], sys.argv[2]
ts = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%d-%H%M%S")
client = storage.Client.from_service_account_json(KEY)
name = f"vps-backups/{net}-{ts}.db"
client.bucket(BUCKET).blob(name).upload_from_filename(src, timeout=2400)
print(f"UPLOADED {name} {os.path.getsize(src)} bytes")
