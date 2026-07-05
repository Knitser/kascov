# kascov — Kaspa Covenant Explorer

**Live dashboard: [kascov-explorer.web.app](https://kascov-explorer.web.app)**

CLI + indexer for **covenants on Kaspa L1**, introduced by the [Toccata hardfork](https://docs.kaspa.org/toccata) (June 30, 2026).

Toccata lets UTXOs carry application state: outputs can be bound to a **covenant ID** ([KIP-20](https://github.com/kaspanet/kips/blob/master/kip-0020.md)) that persists across state transitions, forming an on-chain lineage from a covenant's genesis to its current state UTXO. Nodes validate this — but expose no way to *query* it. There is no "get UTXOs by covenant ID" RPC, and block explorers don't decode covenant data yet.

`kascov` fills that gap:

- **`kascov scan --last N`** — walk recent blocks and dump every covenant-bound output (no database needed)
- **`kascov sync`** — build a local index of all covenant activity, following the virtual selected chain (reorg-aware)
- **`kascov list`** — all known covenants: active / burned, event counts, last activity
- **`kascov show <covenant-id>`** — a covenant's genesis, current state UTXO, and metadata
- **`kascov trace <covenant-id>`** — full lineage: genesis → every state transition → current tip
- **`kascov watch`** — live feed of covenant events as they're accepted

> **Status:** all commands work against live networks. Toccata is live on **mainnet** and **testnet-10** (both supported); Testnet 12 was the pre-fork covenant playground on a separate node branch — not supported. Verified against real covenant traffic on testnet-10 (240+ covenants observed within minutes of scanning). Mainnet covenant traffic is still near zero days after activation — which is exactly why indexing from day one matters.

## Why an index matters

Kaspa nodes prune block data after ~3 days. A covenant's lineage older than that is unrecoverable from a regular node — unless someone indexed it as it happened. `kascov sync --follow` is designed to run continuously so lineage stays complete; covenants first seen mid-life are honestly marked `[history truncated]`.

## Quick start

```sh
# mainnet via the public node resolver (zero setup):
cargo run -p kascov -- scan --last 500

# against your own node (recommended for indexing):
#   kaspad --utxoindex --rpclisten-borsh=0.0.0.0:17110
cargo run -p kascov -- --rpc ws://127.0.0.1:17110 scan --last 500

# testnet-10 (faucet-funded experiments):
cargo run -p kascov -- --network testnet-10 --rpc ws://127.0.0.1:17210 scan --last 500

# machine-readable:
cargo run -p kascov -- --json scan --last 500 | jq .covenant_id
```

## The website

[kascov-explorer.web.app](https://kascov-explorer.web.app) is the hosted face of this index:

- **explorer** — every smart coin with a friendly name, life story timeline, live-updating stats ("watching live" means the indexer saw the chain tip seconds ago; times are exact, UTC on hover). First paint comes from a 30 KB live feed in ~1 s while the full snapshot loads.
- **search that answers the tester's question** — paste a transaction id (or start typing a name for live suggestions) and land on the coin it touched, with that event highlighted; a clear "kascov hasn't seen this" when it isn't covenant traffic
- **watchlist** (★), record holders, sorting; long life stories and UTXO panels fold with expanders
- **[/decode](https://kascov-explorer.web.app/decode)** — paste any script hex, get the post-Toccata disassembly (KIP-17 introspection, KIP-20 covenant ops, KIP-16 zk) in the browser, with a downloadable .txt and an example gallery. It **names compiled SilverScript contracts** (Mecenas, Escrow, LastWill) and labels their constructor arguments — as does the indexer for on-chain states and spend-time reveals. Recognized contracts can be **re-instantiated with your own parameters** ("make this yours"): readable source + rebuilt hex + a one-command testnet deploy via `kascov-lab deploy`.
- **[/dev](https://kascov-explorer.web.app/dev)** — the JSON API documented with curl examples

## The JSON API

An always-on worker (Cloud Run) follows the chain and serves the index as JSON, CORS `*`, no keys:

```sh
# small fast feed: stats + chain tip + newest ~150 events (poll this)
curl -s https://kascov-explorer.web.app/data/testnet-10-live.json | jq .stats

# full snapshot: every covenant, complete timelines, UTXOs with decoded scripts
curl -s https://kascov-explorer.web.app/data/testnet-10.json | jq '.covenants[0]'
```

Field-by-field docs live on the [for developers page](https://kascov-explorer.web.app/#/dev).

## Design notes

- Rust workspace: `kascov-core` (node client, detection, sync, storage), `kascov` (CLI + serve worker), `kascov-decode` (post-Toccata disassembler; `web/disasm.js` is its verified JS port), `kascov-lab` (make real covenants on TN10).
- Kaspa RPC types never leave one module (`node/wrpc.rs`) — the rest of the code uses kascov's own stable model.
- Kaspa crates on crates.io are frozen pre-Toccata; deps are pinned to a single [rusty-kaspa](https://github.com/kaspanet/rusty-kaspa) git rev in the workspace manifest. The pin must be wire-compatible (borsh) with the node you connect to.
- Index storage is SQLite — single file per network, disposable and rebuildable. The hosted worker restores/backs up its DBs via GCS so history survives restarts; `sync` records the chain tip so exports can date events exactly, and prefetches accepting blocks concurrently to outrun busy testnets.
- Deployment: Firebase Hosting serves `web/`; `/data/**` rewrites to the Cloud Run worker (`scripts/deploy-worker.sh`). The old laptop publish loop (`scripts/kascov-live.sh`) is no longer needed for production data.

## License

MIT
