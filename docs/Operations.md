# Operations

This note covers the operational paths around the production worker and static web app. For the machine/process diagram, see [[Architecture#Deployment topology (live since July 22)]]. For the indexer's recovery behavior, see [[Sync Engine]].

## Production release

Production runs from a dedicated Windows server:

- Caddy serves the mirrored `web/` directory;
- the Rust worker runs as `kascov-worker` inside WSL2;
- mainnet and testnet-10 archival `kaspad` services run on the host;
- Caddy proxies data, OpenAPI, stream, image, share, feed, sitemap, and health routes to the worker.

Release procedure:

1. fast-forward `/home/kascov/kascov` to the tested `main` revision;
2. build `kascov --release` as the service user;
3. restart `kascov-worker`;
4. mirror that exact revision's `web/` directory to `C:\kascov\web`;
5. verify `/health`, both pending snapshots, the live feed, the SPA, and a deep link.

Caddy does not need a reload for a normal application release.

### Static module cache rule

The web app has no bundler or hashed output filenames. `index.html` therefore versions the root module URL, and `app.js` versions imported request-layer modules. When a core module changes in a way that must ship atomically with its importer, update the query revision in the same commit. This prevents a browser from combining a new shell with a stale request implementation.

## Daily digest automation

`.github/workflows/digest.yml` runs the daily “today on Kaspa smart coins” digest at `16:00 UTC`. `scripts/digest-post.mjs` fetches:

```text
https://kascov.io/data/mainnet/digest.json
```

and formats births, moves, retirements, value born, active coins, and the busiest coin.

### Canary decision, secrets, and dry runs

The current operational decision is **not to enable Telegram**. The scheduled
workflow is retained as a canary for the public mainnet digest endpoint:
secret-less runs fetch and print the digest, emit a warning, and stay green.
A red run should therefore mean that the endpoint or fetch path is genuinely
unhealthy—not that optional growth plumbing was never configured.

The dormant delivery path supports:

- `TELEGRAM_BOT_TOKEN`
- `TELEGRAM_CHAT_ID`

If either secret is missing, the scheduled workflow emits a GitHub warning,
prints the digest into the job log, and succeeds. Do not add the secrets or
delete the workflow unless that product decision changes.

Manual dispatch defaults to `dry_run=true`. A local preview is:

```bash
node scripts/digest-post.mjs --dry-run
```

Optional environment overrides:

```bash
KASCOV_BASE=http://localhost:8080 \
KASCOV_NETWORK=testnet-10 \
node scripts/digest-post.mjs --dry-run
```

### Retry and duplicate-post policy

Fetching `digest.json` is safe to repeat. The script makes up to three attempts for network failures and `5xx` responses, with a 20-second timeout per attempt and backoff between attempts. A `4xx` fails immediately because retrying cannot repair a bad request.

Telegram delivery is more conservative:

- retry a definite `5xx`;
- do not retry a network error or timeout, because the outcome is ambiguous and a retry could double-post.

The workflow has one `daily-digest` concurrency group and never cancels an in-progress run that may already have posted. Permissions are pinned to `contents: read`; checkout uses a sparse `scripts/` checkout.

Known accepted limitation: a `200` response with a stale `generated_at_ms`
still passes, and the canary currently checks mainnet only.

## First-party traffic measurement

Traffic is measured from privacy-filtered Caddy JSON access logs, not from browser tracking. The canonical instructions and retention policy live in [[Traffic Analytics]].

Operationally important interpretation rules:

- long-lived `/stream` requests count as API calls but not as response-latency samples;
- covenant IDs, transaction-like hexadecimal identifiers, long numeric segments, and address segments are normalized before endpoint ranking;
- obvious scanner paths such as `wp-*`, `xmlrpc.php`, `.env`, `phpmyadmin`, `cgi-bin`, and generic `.php` probes count as bot traffic, not visitors or top pages;
- health probes are separated from human page views;
- first-party API calls are inferred from same-origin fetch metadata or a kascov referrer.

These rules keep p50/p95 latency, visitor counts, and top endpoints useful under ordinary internet background noise.

Run the report tests with:

```bash
python3 -m unittest scripts/test_traffic_report.py
```

## Changelog and feed

Every public milestone is written twice:

- `web/changelog.json` powers the in-app changelog;
- `crates/kascov/assets/changelog.json` is embedded into the Rust worker and powers `/feed.xml`.

The copies must remain byte-for-byte equivalent. Entries describe user-visible capabilities rather than mirroring one commit per line.

The July 23–25 entry groups the reliability work into three milestones:

- a live page that no longer moves under the reader;
- a dense galaxy that streams safely in tiers;
- the builder guide joining the main application shell.

## July 25, 2026 documentation catch-up

The vault's previous substantive update landed with private traffic analytics (`03d5d6a`). This table accounts for every later commit through the guide/changelog work:

| commit | implementation | canonical documentation |
|---|---|---|
| `240bfa6` | keyed refresh gate; one in-flight plus one trailing refresh | [[Web Explorer#Live refresh model]] |
| `919f2d2` | normalize parameterized API paths in traffic reports | [[Traffic Analytics#Endpoint grouping]] |
| `6214e11` | share duplicate live confirmation/feed requests | [[Web Explorer#Request sharing]] |
| `5f38f7b` | bump request-module cache revisions | [[Operations#Static module cache rule]] |
| `9bd84cc` | classify scanner probes as bots, not visitors | [[Traffic Analytics#Bots, scanners, and health probes]] |
| `76acf8a` | accessible route focus without a giant visual ring | [[Web Explorer#View transitions and focus]] |
| `20beb9c` | resilient optional-secret daily digest workflow | [[Operations#Daily digest automation]] |
| `66f5541` | ordered, aligned, rectangular Explore shortcut rail | [[Web Explorer#Explore section navigation]] |
| `8c38cb7` | move the builder guide into the SPA shell | [[Web Explorer#Builder guide inside the shell]] |
| `48d52c1` | catch the public changelog/feed up to July 25 | [[Operations#Changelog and feed]] |

## Verification checklist

Before a production push:

- run `node --test web/*.test.mjs`;
- run `python3 -m unittest scripts/test_traffic_report.py` when analytics code changed;
- run the relevant Rust tests when worker, sitemap, or feed code changed;
- confirm the two changelog JSON files still match;
- verify the root page and a clean deep link through Caddy;
- verify mainnet and testnet-10 health and pending feeds;
- confirm no credentials, local vault state, screenshots, or database files entered the commit.

## Alerting, restore-proof, and the canonical deploy (August 2026)

Three additions under `scripts/ops/`. Like the other ops scripts they are copied onto the box, never executed from the checkout.

### Alerting

`scripts/ops/kascov-alert.sh` posts its single argument as `{"content": …}` to a Discord webhook. It resolves the webhook from `$KASCOV_ALERT_WEBHOOK`, falling back to sourcing `/home/kascov/.kascov-alert.env` (chmod 600, holds `KASCOV_ALERT_WEBHOOK=…`, never in the repo — same pattern as `.deploy-key`). With neither present it logs a loud warning and exits 0: delivery is best-effort by design and must never break a caller.

To arm:

1. copy the script to `/usr/local/bin/kascov-alert.sh` and mark it executable;
2. write the webhook URL into `/home/kascov/.kascov-alert.env`;
3. install `scripts/ops/systemd/kascov-alert@.service` and `systemctl daemon-reload`.

Every other unit now carries `OnFailure=kascov-alert@%n.service`, so any unit entering `failed` posts "unit <name> failed on <host>". The watchdog additionally escalates from its restart branch with the healthz status, the consecutive-failure count, and a since-boot restart counter — a self-heal nobody hears about is how the 49-hour wedge happened.

### Restore-proof

`scripts/ops/kascov-restore-verify.sh` (weekly, `kascov-restore-verify.timer`, `Persistent=true`) downloads the newest `vps-backups/mainnet-*.db` object from GCS, restores it to a scratch path, runs `PRAGMA quick_check`, counts `covenant_events`, and compares against the live DB read-only. Shrink tolerance is 0: the archive only grows, so a restored count above the live count means live history was lost. Any failure — including `$KASCOV_RESTORE_KEY_JSON` being unset — alerts and exits 1, because a restore-proof that silently skips is worse than none.

The box's own GCS key is write-only (`objectCreator`) by design, so this job needs a second, read-scoped service-account key. To arm:

1. create a read-scoped (`objectViewer`) key and place it at `/home/kascov/.gcs-restore-key.json`, chmod 600 (the service points `KASCOV_RESTORE_KEY_JSON` there);
2. copy the script to `/home/kascov/kascov-restore-verify.sh`;
3. install the service and timer, then `systemctl enable --now kascov-restore-verify.timer`.

### Canonical deploy

`scripts/ops/deploy.sh` is the one way to deploy the VPS, written after a hand rebuild silently shipped a stale commit. It hard-resets `/home/kascov/kascov` to `origin/main`, exports `KASCOV_GIT_HASH` before `cargo build --release -p kascov` so the binary embeds the hash, publishes `web/` into `/mnt/c/kascov/web` per file and copy-only (it never deletes there: `ops/traffic/traffic.json` is written into the served root by the traffic timer), restarts `kascov-worker`, then polls `/healthz` for up to 60 s and fails the deploy — with an alert — unless the reported `build` field equals the deployed hash.

Requirements: run as the `kascov` user from outside the checkout (it resets the checkout mid-run), cargo env at `~/.cargo/env`, sudo rights for `systemctl restart kascov-worker.service`. No other environment is needed; the alert webhook is optional but recommended.

## Related notes

- [[Architecture]]
- [[Web Explorer]]
- [[Traffic Analytics]]
- [[Sync Engine]]
- [[Storage Schema]]
