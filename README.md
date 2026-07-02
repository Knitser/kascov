# kascov — Kaspa Covenant Explorer

CLI + indexer for **covenants on Kaspa L1**, introduced by the [Toccata hardfork](https://docs.kaspa.org/toccata) (June 30, 2026).

Toccata lets UTXOs carry application state: outputs can be bound to a **covenant ID** ([KIP-20](https://github.com/kaspanet/kips/blob/master/kip-0020.md)) that persists across state transitions, forming an on-chain lineage from a covenant's genesis to its current state UTXO. Nodes validate this — but expose no way to *query* it. There is no "get UTXOs by covenant ID" RPC, and block explorers don't decode covenant data yet.

`kascov` fills that gap:

- **`kascov scan --last N`** — walk recent blocks and dump every covenant-bound output (no database needed)
- **`kascov sync`** — build a local index of all covenant activity, following the virtual selected chain (reorg-aware)
- **`kascov list`** — all known covenants: active / burned, event counts, last activity
- **`kascov show <covenant-id>`** — a covenant's genesis, current state UTXO, and metadata
- **`kascov trace <covenant-id>`** — full lineage: genesis → every state transition → current tip
- **`kascov watch`** — live feed of covenant events as they're accepted

> **Status:** early days. Toccata is live on **mainnet** and **testnet-10** (both supported); Testnet 12 was the pre-fork covenant playground and ran a separate node branch — not supported. `scan` works; `sync`/`list`/`show`/`trace`/`watch` are in progress.

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

## Design notes

- Rust workspace: `kascov-core` (node client, detection, sync, storage), `kascov` (CLI). Kaspa RPC types never leave one module (`node/wrpc.rs`) — the rest of the code uses kascov's own stable model.
- Kaspa crates on crates.io are frozen pre-Toccata; deps are pinned to a single [rusty-kaspa](https://github.com/kaspanet/rusty-kaspa) git rev in the workspace manifest. The pin must be wire-compatible (borsh) with the node you connect to.
- Index storage is SQLite — single file per network, disposable and rebuildable (TN12 resets happen).

## License

MIT
