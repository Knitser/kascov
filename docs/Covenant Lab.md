# Covenant Lab

`kascov-lab` — we don't just observe covenants, we make them. The lab creates a real covenant lifecycle on **testnet-10** that the explorer then indexes and traces: the end-to-end proof. (Proven July 2: covenant `3af0fffe…` — genesis → 2 transitions → burn, fully traced.)

## Why no SilverScript needed

A covenant is a *consensus* construct (KIP-20), not a language construct. Any v1 transaction output carrying a `CovenantBinding` forms one — the script can be a plain pay-to-pubkey. The lab:

1. **Genesis** — spends a funding UTXO; output 0 carries `CovenantBinding { authorizing_input: 0, covenant_id }` where the id is computed with the consensus helper `covenant_id(outpoint, [(0, &output)])` (BLAKE2b-256, domain `"CovenantID"` — see [[Toccata Protocol Notes]]). The [[Sync Engine#Classification]] now validates exactly this construction when classifying genesis events — the lab and the indexer literally call the same function.
2. **Transitions** — each spends the previous covenant UTXO and re-binds the same id (continuation). The spent UTXO's `covenant_id` is set in the signing entry.
3. **Burn** — spends the covenant UTXO into a plain output. Lineage ends.

Transactions are `TX_VERSION_TOCCATA` (v1), schnorr-signed with rusty-kaspa's own `sign()` helper (which also sets the v1 input compute-budget commitment). Storage mass is left at 0 — the node computes and sets it during mempool validation.

## Running it

```sh
kascov-lab keygen                      # prints a kaspatest: address
# fund it: https://faucet-testnet.kaspanet.io (Cloudflare-gated — use a browser)
kascov-lab balance
kascov-lab demo --transitions 2        # genesis → 2 transitions → burn

# watch kascov catch it (start sync BEFORE the demo to capture all events):
kascov --network testnet-10 sync --follow &
kascov --network testnet-10 trace <covenant-id-from-demo>

# or just paste the demo's txid into the search box at kascov-explorer.web.app —
# it lands on the coin with the event highlighted, live within seconds
```

Key file: `/tmp/kascov-lab-key.hex` (throwaway testnet key). TN10 min relay fee ≈ 166k sompi for a 1-in-1-out tx; the lab uses 500k.

## Design note

The lab deliberately breaks [[Architecture]] Rule 1 (it imports kaspa crates directly) — building and signing transactions *is* the upstream surface, and wrapping it in our model types would only re-state the kaspa API with extra steps.


## Deploying a compiled contract

The generator on [kascov-explorer.web.app/decode](https://kascov-explorer.web.app/decode) turns any recognized SilverScript contract into *your* instance: edit the constructor args, copy the compiled hex, then:

```sh
cargo run -p kascov-lab -- keygen          # prints address, pubkey, blake2b(pubkey)
# fund the address at https://faucet-testnet.kaspanet.io, then:
cargo run -p kascov-lab -- deploy --program-hex <hex> --value 1000000000
```

The coin is born with a **P2SH commitment** state (`OpBlake2b <blake2b-256(program)> OpEqual`) bound to a fresh covenant id, and appears on the explorer within ~a minute. Honesty note: it shows as `p2sh commitment` until a spend reveals the program — and spending means satisfying the contract's own rules, which the lab doesn't automate (yet).
