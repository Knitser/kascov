# CLI Reference

Global flags: `--network mainnet|testnet-10` (default mainnet) · `--rpc ws://…` (default: public resolver) · `--db <path>` (default `~/.kascov/<network>.db`) · `--json` (machine-readable output on every read command).

## kascov

| Command | What it does |
|---|---|
| `scan --last N` | No database: walk N recent blocks backwards from the sink (concurrent BFS over parents) and dump every covenant-bound output. The "is anything happening on this network" tool. |
| `sync [--from <hash>] [--follow]` | Build/update the index ([[Sync Engine]]). `--follow` keeps running (2s poll, concurrent block prefetch). Records the chain tip each pass so exports date events exactly. |
| `list [--limit N]` | Indexed covenants: status, events, live UTXOs, value, lineage completeness. |
| `show <covenant-id> [--decode]` | Genesis, status, live state UTXOs. `--decode` disassembles the state script **and any program revealed at spend** ([[Decoding]]). |
| `trace <covenant-id>` | Full lineage with txid, DAA, accepting block — plus the revealed state **payload per event and the payload Δ** between consecutive reveals. |
| `watch` | Live covenant event feed (`--json` = line-delimited JSON, pipe to `jq`). |
| `export [--out <file>] [--max-events N]` | Write the web snapshot (`web/data/<network>.json`) **and** the small live feed (`…-live.json`): stats, tip anchor, newest events. |
| `serve --listen <addr> [--networks a,b] [--db-dir <dir>]` | The always-on worker: follows each network and serves the whole JSON API over HTTP (CORS `*`, gzip/brotli). What runs on Cloud Run ([[Architecture#Deployment topology (live since July 2)]]). |
| `backup --out <file>` | Consistent DB copy (`VACUUM INTO`), safe while syncing — used for GCS continuity. |
| `reset --yes` | Drop the index DB. |

### `serve` endpoints

Everything under `/data/{network}/…` unless noted. Full request/response docs with curl examples live on [kascov.io/#/dev](https://kascov.io/#/dev).

- **Feeds** — `/data/{net}.json` (grid: 20k-row first page, `next_after_daa`/`next_after_id` cursors, `?after_daa=&after_id=&limit=`), `/data/{net}-live.json` (stats + tip + ~150 newest events), `stream` (SSE).
- **Detail** — `c/{id}` (full timeline, UTXOs, `holders[]`, zk fields), `tx/{txid}`, `addr/{address}`.
- **Analytics** — `digest.json`, `templates.json`, `activity.json`, `families.json`, `galaxy.json` (`?fmt=2&tier=core`), `lanes.json`, `lane/{ns}`, `inscriptions.json`, `lifespans.json`, `reorgs.json`.
- **Find** — `search?q=&limit=` (id prefix / name prefix / template substring), `debug/{txid}` (real-witness replay, trace capped at 2000 opcodes).
- **Write** — POST `simulate`, `zk-verify`, `compile`, `publish` (+ GET `verified/{hash}`), `subscribe`/`unsubscribe` (webhooks with real SSRF-guarded delivery), `deploy` (custodial; only when `KASCOV_DEPLOY_KEY` is set and network is testnet-10, rate-limited).
- **Share surface** (site root, not `/data`) — `/share/{net}/{id}`, `/og/{net}/{id}.png` (1200×630 card), `/sitemap.xml`; plus `/healthz`.

## kascov-lab ([[Covenant Lab]])

| Command | What it does |
|---|---|
| `keygen` | Generate a keypair (`/tmp/kascov-lab-key.hex`), print the testnet address + pubkey + blake2b(pubkey). |
| `balance` | Address + UTXO count + balance on TN10. |
| `examples` | Print every copy-paste recipe (no key or network needed). |
| `demo [--transitions N]` | Full covenant lifecycle: genesis → N transitions → burn. |
| `contract-demo` | One command: deploy a Mecenas, then reclaim it — revealed on kascov. |
| `escrow-demo` | One command: deploy an escrow, then settle it to the buyer. |
| `deploy --program-hex <hex> --value <sompi>` | Birth a compiled contract as a real covenant (hidden P2SH commitment state). |
| `spend --program-hex <hex> --entrypoint <e>` | Satisfy an entrypoint and reveal the program on-chain. Entrypoints: `reclaim` \| `cold` \| `inherit` (pure-signature) \| `receive` \| `refresh` (output-constrained, via `spend_constrained`). |
| `settle-escrow --program-hex <hex> --release-to buyer\|seller` | Satisfy `Escrow.spend` as arbiter, forcing the payout to a party. |

## Session-proven examples

```sh
# mainnet, zero setup (public resolver):
kascov scan --last 500

# TN10 covenant traffic:
kascov --network testnet-10 scan --last 5000

# index TN10 live, then inspect:
kascov --network testnet-10 sync --follow &
kascov --network testnet-10 list
kascov --network testnet-10 show <covenant-id> --decode
kascov --network testnet-10 trace <covenant-id>     # payload Δ appears on P2SH covenants

# the hosted API (same data, no local setup):
curl -s https://kascov.io/data/testnet-10-live.json | jq .stats
```
