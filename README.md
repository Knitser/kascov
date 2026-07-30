# kascov — the covenant explorer for Kaspa

**Live: [kascov.io](https://kascov.io)** · Rust indexer + CLI · open JSON API, no keys · MIT

Kaspa's [Toccata hardfork](https://docs.kaspa.org/toccata) (June 30, 2026) let UTXOs carry rules. An output can be bound to a **covenant ID** ([KIP-20](https://github.com/kaspanet/kips/blob/master/kip-0020.md)) that survives being spent, so a coin has a *lineage*: genesis → every state transition → its current tip. Nodes validate that lineage. Nothing exposes it — there is no "get UTXOs by covenant ID" RPC, and general-purpose block explorers don't decode covenant data.

kascov reads it from the chain itself and offers it three ways: a **CLI**, an **open JSON API**, and a **web explorer**. Every fact it publishes is derived from consensus data or from programs the chain revealed at spend time. There is no registry, no allowlist, no trusted submission path — if kascov claims a coin is a token, you can re-derive that from the same bytes.

## What's indexed right now

| network | covenants | active | events | tokens |
|---|---:|---:|---:|---:|
| **mainnet** | 1,851 | 90 | 6,488 | 5 |
| **testnet-10** | 510,681 | 55,084 | 2,313,144 | 461 |

<sub>As of July 26, 2026. `curl -s https://kascov.io/healthz` for current sync state; the live feeds below carry the current counts.</sub>

Both networks are followed continuously, beside kascov's own archival Kaspa nodes. Testnet 12 was the pre-fork covenant playground on a separate node branch and is **not** supported.

## Why an index matters

At 10 blocks per second a Kaspa node retains roughly **30 hours** of prunable consensus data. (The older "~3 days" figure predates Crescendo.) Past that horizon, a covenant's earlier history cannot be recovered from a regular node — unless something indexed it while it happened.

`kascov sync --follow` is built to run forever for exactly that reason. A covenant first seen mid-life is marked `[history truncated]` rather than presented as complete, and a lineage kascov cannot prove back to a KIP-20 genesis is reported as incomplete instead of being given a fabricated origin.

## Quick start

Rust stable, no other prerequisites.

```sh
# mainnet through the public node resolver — zero setup
cargo run -p kascov -- scan --last 500

# against your own node (recommended for indexing)
#   kaspad --utxoindex --rpclisten-borsh=0.0.0.0:17110
cargo run -p kascov -- --rpc ws://127.0.0.1:17110 scan --last 500

# testnet-10, where most covenant traffic lives
cargo run -p kascov -- --network testnet-10 --rpc ws://127.0.0.1:17210 scan --last 500

# build a local index, then read it
cargo run -p kascov -- --network testnet-10 sync --follow
cargo run -p kascov -- --network testnet-10 list --limit 20

# machine-readable anywhere
cargo run -p kascov -- --json scan --last 500 | jq .covenant_id
```

Global flags: `--rpc <ws-url>` (defaults to the public resolver), `--network mainnet|testnet-10`, `--json`, `--db <path>` (default `~/.kascov/<network>.db`).

## The CLI

| command | what it does |
|---|---|
| `scan --last N` | Walk N recent blocks back from the sink and dump every covenant-bound output. No database. The "is anything happening here" tool. |
| `sync [--from <hash>] [--follow]` | Build or update the index by following the virtual selected chain. Reorg-aware; `--follow` runs continuously and prefetches accepting blocks concurrently. |
| `list [--limit N]` | Every indexed covenant: status, event count, live UTXOs, value, lineage completeness. |
| `show <id> [--decode]` | One covenant's genesis, status and live state UTXOs. `--decode` disassembles the state script *and* any program revealed at spend. |
| `trace <id>` | Full lineage — txid, DAA, accepting block per event, plus the revealed payload and the payload delta between consecutive reveals. |
| `inspect-tx <txid>` | A transaction's whole covenant anatomy: bindings, compute budgets, payload lanes. The tiebreaker for classification disputes. |
| `watch` | Live event feed as covenants are accepted (`--json` gives line-delimited JSON). |
| `export [--out <file>] [--max-events N]` | Write the web snapshot plus the small live feed: stats, tip anchor, newest events. |
| `serve [--listen addr] [--networks a,b] [--db-dir <dir>]` | The always-on worker: follows each network and serves the entire JSON API over HTTP (CORS `*`, gzip/brotli). Defaults to `0.0.0.0:8080`. |
| `backup --out <file>` | Consistent database copy via `VACUUM INTO` — safe to run while syncing. |

Full reference with examples: [`docs/CLI Reference.md`](docs/CLI%20Reference.md).

## Make your own smart coin

`kascov-lab` builds real covenants on testnet-10 — not simulations. One command births a contract and then spends it under its own rules, and the coin reveals itself on kascov: named, arguments labeled, permanent.

```sh
cargo run -p kascov-lab -- keygen          # a TN10 key + address (fund it at the faucet)
cargo run -p kascov-lab -- examples        # every copy-paste recipe, no key needed

cargo run -p kascov-lab -- contract-demo   # deploy a Mecenas, then reclaim it
cargo run -p kascov-lab -- escrow-demo     # deploy an escrow, then settle it to the buyer
```

Lower-level steps — `deploy`, `spend --entrypoint <reclaim|cold|inherit|receive|refresh>`, `settle-escrow --release-to buyer|seller`, `demo --transitions N` — let you drive the lifecycle yourself. To choose the contract parameters, use the generator on [kascov.io/decode](https://kascov.io/decode) ("make this yours"), then `deploy` → `spend`.

Two operational tools live here too: `probe-block` (is a below-pruning-point block actually servable by this node?) and `recover-gap` (merge a skipped DAA window into an offline copy of the index by walking a node's own archival history).

The 15-minute path end to end, including the compute-budget trap that stops most first attempts: **[kascov.io/guide](https://kascov.io/guide)**. Deeper notes: [`docs/Covenant Lab.md`](docs/Covenant%20Lab.md).

## The website

[kascov.io](https://kascov.io) is a static SPA over the same public API — no build step, no framework.

- **[explore](https://kascov.io/explore)** — every smart coin with a friendly name, a life-story timeline, and live-updating stats. First paint comes from a ~30 KB live feed while the full snapshot loads. Includes the **galaxy**: every coin and the moves between them, streamed in tiers so a dense network draws fast and stays readable.
- **coin, address and transaction pages** — holders, decoded payloads, spend cost, provable genesis, and a spend you can replay opcode by opcode.
- **[tokens](https://kascov.io/tokens)** — the covenant-token directory with a deliberately conservative verdict: `verified` only when every event in a token's history matched a known KCC20 rule and supply is conserved. Anything ambiguous stays `unvalidated` with a reason, never a false "valid". Token art is rendered only when its bytes match the SHA-256 the deployer pinned at genesis.
- **[playground](https://kascov.io/decode)** — paste any script hex and read it as opcodes, including the post-Toccata set: introspection ([KIP-17](https://github.com/kaspanet/kips/blob/master/kip-0017.md)), covenant ops (KIP-20), zk verification ([KIP-16](https://github.com/kaspanet/kips/blob/master/kip-0016.md)). Recognizing, disassembling and linting run in your browser. It names compiled SilverScript contracts (Mecenas, Escrow, LastWill) and labels their constructor arguments — as does the indexer, for on-chain states and spend-time reveals.
- **[builder](https://kascov.io/build)** — a no-code path to a real covenant, with one-click testnet-10 deploy (server-side custodial key, gated by `KASCOV_DEPLOY_KEY`).
- **[preflight](https://kascov.io/preflight)** — paste a transaction as JSON and find out whether it passes before you broadcast: per-input compute budgets against the classic "script units exceeded" trap, masses against the block limit, an honest fee estimate.
- **[API](https://kascov.io/dev)** — the JSON API documented field by field with working curl examples.
- **[changelog](https://kascov.io/changelog)** — what kascov learned to do, newest first. Also an [Atom feed](https://kascov.io/feed.xml).

The whole site is plain files: every page is a `<section>` in `index.html` that the hash router unhides, so there is nothing to compile and every route is readable in the source.

## The JSON API

One always-on worker serves the index through the same origin as the site. CORS `*`, no keys, no rate cards.

```sh
# small fast feed: stats + chain tip + newest ~150 events (poll this)
curl -s https://kascov.io/data/mainnet-live.json | jq .stats

# the grid: stats + one summary row per covenant
curl -s https://kascov.io/data/mainnet.json | jq '.covenants[0]'

# what this worker offers, per network
curl -s https://kascov.io/data/mainnet/index.json | jq .endpoints
```

| endpoint | returns |
|---|---|
| `/data/{net}-live.json` | Stats, tip anchor, newest events. The cheap poll. |
| `/data/{net}.json` | The grid: one summary row per covenant. Paginated (below). |
| `/data/{net}/c/{id}` | One coin: events, UTXOs, holders, decoded state, provable genesis. |
| `/data/{net}/coins?ids=` | Batch coin summaries. |
| `/data/{net}/events?after_daa=&after_seq=&limit=` | The raw event log, forward-paginated. |
| `/data/{net}/tx/{txid}`, `/data/{net}/addr/{address}` | Transaction and address views. |
| `/data/{net}/tokens.json`, `/data/{net}/token/{id}` | Token directory and one token's whole story, with validation verdicts. |
| `/data/{net}/search?q=` | Ids, friendly names and templates. |
| `/data/{net}/galaxy.json?tier=core&fmt=2` | Network graph geometry. `tier=core` returns only the larger clusters for a fast first paint; `fmt=2` switches to a columnar shape (parallel arrays instead of per-node objects), and `tier=visual` adds the outer geometry as a small delta over core. |
| `/data/{net}/lanes.json`, `/data/{net}/lane/{ns}` | Payload lanes — inscriptions and namespace tags with their own volumes. |
| `/data/{net}/templates.json`, `/data/{net}/template/{hash}` | Recognized contract families, addressable by KCC-1 draft TemplateHash. |
| `/data/{net}/activity.json`, `families.json`, `lifespans.json`, `inscriptions.json`, `reorgs.json`, `digest.json`, `consistency.json` | Analytics, plus the indexer's own reorg feed and a daily cross-indexer consistency report. |
| `/data/{net}/stream` | Server-sent events, same-origin, unbuffered. |
| `/data/{net}/pending` | Live mempool covenant activity, with explicit poller health so an empty strip is never ambiguous. |
| `/data/{net}/simulate`, `debug/{txid}`, `zk-verify`, `compile`, `publish` | Off-chain script-engine execution, replay, proof verification, contract compilation. |
| `/data/{net}/subscribe`, `unsubscribe` | Webhooks (`{url, covenant_id?, kind?}`), delivered with SSRF guards. |
| `/share/{net}/{id}`, `/og/{net}/{id}`, `/badge/{net}/{id}`, `/img/{net}/{id}` | Shareable coin page, rendered OG PNG, SVG badge, hash-verified token art. |
| `/healthz`, `/sitemap.xml`, `/feed.xml` | Sync state per network, crawlable coin index, changelog feed. |

**Pagination.** A bare grid request returns a first page capped at 20,000 rows, newest activity first. When more remain, the response carries `next_after_daa` + `next_after_id`; pass them back as `?after_daa=&after_id=&limit=` to keep walking (default page 5,000). `/events` uses the same shape with `after_seq`.

Zero-dependency clients (Node and Python, single file each, stdlib only) are in [`clients/`](clients/). Field-by-field docs: [kascov.io/dev](https://kascov.io/dev).

## Standards and conventions

kascov implements the consensus-level KIPs — [KIP-16](https://github.com/kaspanet/kips/blob/master/kip-0016.md) (zk verification), [KIP-17](https://github.com/kaspanet/kips/blob/master/kip-0017.md) (introspection), [KIP-20](https://github.com/kaspanet/kips/blob/master/kip-0020.md) (covenants) — and tracks the [Kaspa Calls for Conventions](https://github.com/kaspanet/kccs) track for everything above consensus, where independent implementations benefit from converging:

- **[KCC-0001](https://github.com/kaspanet/kccs/pull/3)** (draft) — covenant definition, byte layout, program ABI. kascov surfaces §8.3 TemplateHash identities per contract family, addressable at `/data/{net}/template/{hash}`.
- **[KCC-0020](https://github.com/kaspanet/kccs/pull/2)** (draft) — fungible token behavior. kascov's token accounting derives supply, holders and per-event classification from chain bytes alone, and refuses to call a token valid unless every event matched a known rule.
- **[KCC-0021](https://github.com/kaspanet/kccs/pull/6)** (draft, from this project) — covenant token metadata: one JSON object in the genesis payload giving a token its name, ticker, decimals and a hash-committed image. This is the convention behind kascov's verified token art.

Conventions are read, never assumed: an unrecognized shape is reported as unrecognized.

## Repository layout

| path | contents |
|---|---|
| `crates/kascov-core` | Node client boundary, covenant detection, the sync engine, SQLite storage, token accounting. |
| `crates/kascov-decode` | The post-Toccata disassembler and template recognition. `web/disasm.js` is its verified JS port — byte-identical output on every indexed script. |
| `crates/kascov-sim` | Off-chain script-engine harness behind simulate / debug / zk-verify. |
| `crates/kascov-labkit` | Covenant transaction building and signing, shared by the lab CLI and the worker's one-click deploy. |
| `crates/kascov-lab` | Operator-facing testnet workflows: deploy, spend, settle, recover. |
| `crates/kascov` | The CLI and the `serve` worker (axum). |
| `web/` | The static SPA: `index.html`, `app.js`, `core/*` (state, data, loading, routing, refresh), plus `*.test.mjs`. |
| `clients/` | Zero-dependency JS and Python API wrappers. |
| `scripts/` | Caddy configs, VPS provisioning, digest posting, traffic reporting, lineage repair. |
| `docs/` | Architecture, sync engine, storage schema, decoding, protocol notes, roadmaps. |

## Development

```sh
cargo build --workspace
cargo test --workspace                              # model, decoder, store, sync, worker, labkit, sim
cargo test -p kascov-core --test sync_replay        # reorg + lineage convergence on scripted chains
cargo test -p kascov-core --test gap_recovery       # window merge, resequencing, idempotence
npm ci && npm run test:web                          # Node contracts + real Chrome responsive behavior
python3 -m unittest scripts/test_traffic_report.py  # log-parsing privacy rules
```

None of this needs a Kaspa node. `ChainSource` is a boundary trait, so the sync engine is driven by scripted virtual chains against temporary SQLite stores — including reorgs, rollback and re-acceptance convergence. The web tests are dependency-free and assert against the real `web/` sources rather than a copy.

**Previewing the site.** `web/` is static, but the pages fetch worker routes, so a bare file server leaves them empty. Run the worker and put the static files in front of it with `/data/**`, `/share/**`, `/og/**`, `/badge/**`, `/img/**`, `/sitemap.xml`, `/feed.xml` and `/healthz` proxied through — `scripts/kascov.Caddyfile` is the reference for exactly which paths those are, and its `try_files {path} {path}.html {path}/ /index.html` is what makes clean URLs like `/guide` reach the hash router. Pointing the same proxy at `https://kascov.io` instead of a local worker is enough to work on the frontend alone.

## Running your own instance

```sh
cargo run --release -p kascov -- serve --listen 0.0.0.0:8080 --networks mainnet,testnet-10
```

The worker owns one SQLite file per network (`~/.kascov/<network>.db` by default) — disposable and rebuildable from the chain, though rebuilding cannot recover history that has since been pruned. Put a reverse proxy in front to serve `web/` as static files and forward `/data/**`, `/share/**`, `/og/**`, `/badge/**`, `/img/**`, `/sitemap.xml`, `/feed.xml` and `/healthz` to the worker; `scripts/kascov.Caddyfile` is the reference. Point it at your own `kaspad --utxoindex` for anything you intend to keep.

Production runs beside kascov's own archival mainnet and testnet-10 nodes: Caddy serves the static site and proxies the worker over the host bridge. The Firebase and Cloud Run configs in this repo remain only as migration history.

## Design notes

- Kaspa RPC types never leave one module (`kascov-core/src/node/wrpc.rs`). Everything downstream operates on kascov's own stable model, so an upstream type change can't ripple through the codebase.
- The Kaspa crates on crates.io are frozen pre-Toccata, so dependencies are pinned to a single [rusty-kaspa](https://github.com/kaspanet/rusty-kaspa) git rev in the workspace manifest. That pin must stay wire-compatible (borsh) with the node you connect to — bump the two together.
- When sources disagree, kascov prefers consensus-accepted chain data, then locally persisted accepted history, then deterministic derivations from it, then verified external bytes (art matching an on-chain hash), and only last an explicitly labeled claim such as a deployer-provided name. Claims are always shown as claims.
- Traffic measurement comes from kascov's own server logs — no analytics SDK, no cookie, no tracking pixel, no third-party collector. See [`docs/Traffic Analytics.md`](docs/Traffic%20Analytics.md).

## Docs

[`docs/Home.md`](docs/Home.md) is the index. Highlights: [Architecture](docs/Architecture.md) · [Sync Engine](docs/Sync%20Engine.md) · [Storage Schema](docs/Storage%20Schema.md) · [Decoding](docs/Decoding.md) · [CLI Reference](docs/CLI%20Reference.md) · [Covenant Lab](docs/Covenant%20Lab.md) · [Toccata Protocol Notes](docs/Toccata%20Protocol%20Notes.md).

## Contributing

Issues and pull requests are welcome — bug reports about a coin kascov classified wrongly are especially useful, since a misclassification is a decoder bug with a reproducible on-chain witness. Include the network and the covenant id or txid; `kascov inspect-tx <txid>` prints everything needed to reason about it.

If you build a Kaspa indexer, wallet or explorer and our numbers disagree, please open an issue. The `/data/{net}/consistency.json` report exists precisely so cross-implementation differences surface as facts rather than arguments.

Run `cargo test --workspace` and `npm run test:web` before opening a PR.

## License

MIT — see [LICENSE](LICENSE).
