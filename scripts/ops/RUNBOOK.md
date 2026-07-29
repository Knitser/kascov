# kascov ops runbook

Everything kascov needs to stay alive, written down because it previously lived
only in one person's head and one chat log. The DB is the sole archive of
mainnet covenant history: public nodes prune at roughly 30 hours, so history
lost here is lost everywhere.

## The box

One VPS, `157.90.7.39`.

- **Windows host** runs the two Kaspa nodes as NSSM services and Caddy
  (`C:\tools\caddy.exe`, config `C:\caddy\Caddyfile`).
- **WSL Ubuntu** runs the worker as systemd `kascov-worker.service`, which
  execs `/home/kascov/kascov-run.sh`.
- Caddy serves static files from `C:/kascov/web` and reverse-proxies the worker
  on `127.0.0.1:8080`. It also fronts `fees.kascov.io`, `rewind.kascov.io` and
  `ironwood.live`, so a bad Caddy change takes down more than kascov.
- DB lives on native ext4 at `/home/kascov/kascov-db`. It must never move back
  to a `/mnt/*` DrvFs path: random I/O there is catastrophically slow and it
  once made snapshot queries take 20+ seconds.

Run privileged commands as:

```bash
ssh Administrator@157.90.7.39 "wsl -d Ubuntu -u root -- bash /mnt/c/Users/Administrator/<script>.sh"
```

Write a script and scp it first. Windows `cmd` mangles quotes and pipes, so
inline one-liners fail in confusing ways.

## Worker environment

Env lives in `/home/kascov/kascov-run.sh`, NOT in the systemd unit and NOT in
the repo, so `git pull` never touches it. The Cloud Run to VPS move silently
dropped two of these and nobody noticed until the features were reported broken:

- `KASCOV_RPC_MAINNET` / `KASCOV_RPC_TESTNET_10` — node wRPC endpoints.
- `SILVERC_BIN` — the SilverScript compiler. Unset means `/compile` and
  `/publish` answer "compiler isn't available".
- `KASCOV_DEPLOY_KEY` — read from `/home/kascov/.deploy-key` when present.
  Absent means `/deploy` returns 404 by design. The file holds a funded
  testnet-10 secret as 64 hex chars, `chmod 600`.

## Rebuild and restart

```bash
# as the kascov user
cd ~/kascov && git pull && cargo build --release -p kascov
sudo systemctl restart kascov-worker.service
```

For long builds use `systemd-run --unit=kascov-rebuild --uid=kascov` so the
build survives the ssh session ending.

## Health and the watchdog

`GET /healthz` returns 200 when both followers are progressing and 503 when one
stalls. **Caddy serves the SPA from disk independently of the worker**, so the
homepage still returns 200 with the worker dead: never use `/` as an uptime
check, always `/healthz`.

`kascov-watchdog.timer` runs every 60s and restarts the worker after 3
consecutive unhealthy checks, with a 15 minute cooldown so it cannot flap. This
exists because a follower once wedged for 49 hours unnoticed and the recovered
history was only partial.

```bash
journalctl -u kascov-watchdog.service -n 20     # what it has been seeing
```

## Backups

| Job | Cadence | What |
|---|---|---|
| `kascov-backup.timer` | 6h | Local snapshot of both DBs, `PRAGMA quick_check`, keeps 12 |
| `kascov-mainnet-fast.timer` | 5 min | Mainnet off-site to GCS, **only when covenant history changed** |
| `kascov-offsite.timer` | 24h | Both DBs off-site to GCS |

The 5 minute job gates on a logical fingerprint (`COUNT(*)` and `MAX(accepting_daa)`
of `covenant_events`), not on file bytes. The follower rewrites its sync cursor
every block, so a byte hash would ship ~157MB every 5 minutes to record nothing
new. Idle cycles cost one indexed query.

Off-site target is `gs://kascov-explorer-index/vps-backups/` using a
**write-only** service account (`objectCreator`) at
`/home/kascov/.gcs-backup-key.json`. A compromised box can add copies but
cannot delete history.

## Restore

**The trap that will bite you:** swapping a SQLite DB while leaving stale `-wal`
and `-shm` sidecars in place produces `database disk image is malformed` and
wedges the follower. Always remove all three.

```bash
sudo systemctl stop kascov-worker.service
DB=/home/kascov/kascov-db/mainnet.db
rm -f "$DB" "$DB-wal" "$DB-shm"          # <- the sidecars matter
cp /home/kascov/kascov-backups/mainnet-latest.db "$DB"
chown kascov:kascov "$DB"
python3 -c "import sqlite3;print(sqlite3.connect('file:$DB?mode=ro',uri=True).execute('PRAGMA quick_check').fetchone())"
sudo systemctl start kascov-worker.service
curl -s localhost:8080/healthz
```

To pull an off-site copy instead, list `gs://kascov-explorer-index/vps-backups/`
and take the newest `mainnet-*.db`.

## Firewall

Node P2P (16111, 16211) is open to the internet on purpose: that is how the
nodes peer. Node **RPC** (17110, 17210) is scoped to `172.16.0.0/12` so only WSL
can reach it. The wide private range rather than the exact WSL subnet is
deliberate, because WSL2 can renumber across reboots and an exact rule would
lock the worker out of its own nodes.

Verify from outside:

```bash
for p in 17110 17210 16111 16211; do nc -z -G 5 157.90.7.39 $p && echo "$p OPEN" || echo "$p closed"; done
```

Expected: 17110 and 17210 closed, 16111 and 16211 open.

### Remote access

**Nothing proxies kascov.io.** Every hostname (kascov.io, fees, rewind, pulse,
ironwood.live) resolves straight to this box and Caddy terminates TLS itself.
There is no edge WAF, no rate limiting, no origin cloaking. Never restrict
80/443 to a CDN's ranges while that holds — it black-holes every site here.
It also means the box's address is public by construction, so security has to
come from the ports being shut, not from the address being unknown.

The state to keep true, and the scripts that restore it:

| port | who may reach it | why |
| --- | --- | --- |
| 22 SSH | anyone, **key only** | a key cannot be brute-forced; this is the fallback that makes everything else safe to lock |
| 3389 RDP | admin IP only | no key auth exists for RDP, and the admin username is public in this repo's history |
| 5985 WinRM | LocalSubnet | remote management, never needed from the internet |
| 16111/16211 P2P | anyone | node peering |
| 17110/17210 RPC | 172.16.0.0/12 | WSL worker only |
| 80/443 | anyone | there is no proxy in front; see above |

The three `scripts/ops/harden-*.ps1` scripts restore this state. They are
**deliberately gitignored**: they map how admin access is locked down, which is
intel an attacker wants and nobody else needs. They live on the workstation and
on the VPS admin home, never in the repo.

- `harden-1-ssh-keyonly.ps1` — key-only SSH. Validates with `sshd -T` and
  self-restores its backup on a bad parse. Run this first; RDP is the fallback
  while it runs.
- `harden-2-remote-access.ps1` — scopes RDP to the IP of the live SSH session
  and WinRM to LocalSubnet. Refuses to run unless SSH is already key-only.
- `harden-3-allow-rdp-from.ps1` — the ISP rotates this box's admin address
  (two different ranges inside one fortnight), so when RDP goes quiet, SSH in
  and run this to re-point it at wherever you are now.

Audit trail for why this matters (2026-07-29): 4,972 failed logons in 14 days,
505 of them against the admin account by name, 120+ source IPs auto-blocked by
RDPGuard. No successful unauthorized logon — every remote success was the admin
account from a Thai ISP address, i.e. you.

## Caddy

```bash
C:\tools\caddy.exe validate --config C:\caddy\Caddyfile --adapter caddyfile
C:\tools\caddy.exe reload   --config C:\caddy\Caddyfile --adapter caddyfile
```

Reload is graceful and validates first. Always validate before reloading: this
config also serves two other people-facing sites.

The `@worker` path matcher must list every worker route. `/healthz` was missing
once and silently served the SPA instead of health JSON. `try_files` must keep
`{path}.html` or clean URLs like `/guide` fall through to the SPA shell.

## Do not run

`scripts/deploy-worker.sh` is retired Cloud Run tooling. Running it today
rewrites the backup bucket lifecycle, redeploys a worker that restores a
months-stale object, and repoints hosting at it. It is kept only for history.
