# CLI Reference

Global flags: `--network mainnet|testnet-10` (default mainnet) · `--rpc ws://…` (default: public resolver) · `--db <path>` (default `~/.kascov/<network>.db`) · `--json` (machine-readable output on every read command).

## kascov

| Command | What it does |
|---|---|
| `scan --last N` | No database: walk N recent blocks backwards from the sink (concurrent BFS over parents) and dump every covenant-bound output. The "is anything happening on this network" tool. |
| `sync [--from <hash>] [--follow]` | Build/update the index ([[Sync Engine]]). `--follow` keeps running (2s poll). |
| `list [--limit N]` | Indexed covenants: status, events, live UTXOs, value, lineage completeness. |
| `show <covenant-id> [--decode]` | Genesis, status, live state UTXOs. `--decode` disassembles the state script ([[Decoding]]). |
| `trace <covenant-id>` | Full lineage: `#000 genesis → #001 transition → … → burn`, each with txid, DAA score, accepting chain block. |
| `watch` | Live covenant event feed (`--json` = line-delimited JSON, pipe to `jq`). |
| `reset --yes` | Drop the index DB. |

## kascov-lab ([[Covenant Lab]])

| Command | What it does |
|---|---|
| `keygen` | Generate a keypair (`/tmp/kascov-lab-key.hex`), print the testnet address. |
| `balance` | Address + UTXO count + balance on TN10. |
| `demo [--transitions N]` | Full covenant lifecycle: genesis → N transitions → burn. |

## Session-proven examples

```sh
# mainnet, zero setup (public resolver):
kascov scan --last 500

# TN10 covenant traffic (this found 242 covenants in 5000 blocks):
kascov --network testnet-10 scan --last 5000

# index TN10 live, then inspect:
kascov --network testnet-10 sync --follow &
kascov --network testnet-10 list
kascov --network testnet-10 show <covenant-id> --decode
kascov --network testnet-10 trace <covenant-id>
```
