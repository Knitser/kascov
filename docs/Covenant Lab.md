# Covenant Lab

`kascov-lab` — we don't just observe covenants, we make them. The lab creates a real covenant lifecycle on **testnet-10** that the explorer then indexes and traces: the end-to-end proof.

## Why no SilverScript needed

A covenant is a *consensus* construct (KIP-20), not a language construct. Any v1 transaction output carrying a `CovenantBinding` forms one — the script can be a plain pay-to-pubkey. The lab:

1. **Genesis** — spends a funding UTXO; output 0 carries `CovenantBinding { authorizing_input: 0, covenant_id }` where the id is computed with the consensus helper `covenant_id(outpoint, [(0, &output)])` (BLAKE2b-256, domain `"CovenantID"` — see [[Toccata Protocol Notes]]).
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
```

Key file: `/tmp/kascov-lab-key.hex` (throwaway testnet key).

## Design note

The lab deliberately breaks [[Architecture]] Rule 1 (it imports kaspa crates directly) — building and signing transactions *is* the upstream surface, and wrapping it in our model types would only re-state the kaspa API with extra steps.
