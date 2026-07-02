# Toccata Protocol Notes

The **Toccata** hardfork made Kaspa L1 programmable. Announced by [@kaspaunchained](https://x.com/kaspaunchained/status/2071999597036101937): *"Toccata is live! $KAS became ultra programmable today!"*

## What it bundles

| KIP | What it adds |
|---|---|
| KIP-17 | Extended script opcodes — transaction introspection (0xb2–0xc9), the covenants backbone |
| KIP-20 | **Covenant IDs** — consensus-level lineage tracking for stateful UTXOs |
| KIP-16 | ZK opcodes — Groth16 + RISC Zero verification via `OpZkPrecompile` |
| KIP-21 | Partitioned sequencing commitments for based ZK apps |
| Tx v1 | `TX_VERSION_TOCCATA = 1`: covenant output bindings, input compute budgets, gas commitments |

## KIP-20 mechanics (what kascov indexes)

- A transaction output may carry a `CovenantBinding { authorizing_input: u16, covenant_id: Hash }`.
- **Genesis:** a new covenant id = BLAKE2b-256 (domain tag `"CovenantID"`) over the authorizing input's outpoint + the ordered list of authorized outputs (index, value, script pubkey — *excluding* the binding itself). Implementation: `consensus/core/src/hashing/covenant_id.rs`; the [[Covenant Lab]] calls this helper directly.
- **Continuation:** an output's covenant id must match the covenant id of the UTXO spent by its authorizing input.
- **Burn:** a covenant UTXO is spent with no successor output claiming the id.
- Spent covenant UTXOs are visible in RPC as `UtxoEntry.covenant_id: Option<Hash>`.
- State lives **in the script itself** (P2SH-style: `OpBlake2b <32B hash> OpEqual` commitment observed on live TN10 covenants); the logic/preimage is revealed at spend time in the signature script. See [[Decoding#Spend-time decoding]].

## Networks

| Network | Toccata | Notes |
|---|---|---|
| mainnet | ✅ active since DAA 474,165,565 (June 30, 2026) | covenant traffic ≈ zero as of July 2 — 20k blocks scanned, none found |
| testnet-10 | ✅ active since DAA 467,579,632 | **live covenant traffic**: 242 covenants seen in ~8 min of blocks |
| testnet-12 | ⚠️ legacy | the pre-fork covenant playground; runs a separate `tn12` node branch with `ForkActivation::never()` on mainnet params. SilverScript's "TN12 only" README note is stale. |

Node: rusty-kaspa **v2.0.1** (mainnet + TN10). Default borsh wRPC ports: mainnet `17110`, testnet `17210`. Public resolver serves mainnet + TN10, **not** TN12.

## Ecosystem

- **SilverScript** ([kaspanet/silverscript](https://github.com/kaspanet/silverscript)) — the covenant language; compiles to native Kaspa Script via `silverc`.
- **vprogs** ([kaspanet/vprogs](https://github.com/kaspanet/vprogs)) — Rust framework for based ZK apps (off-chain compute, on-chain RISC Zero proof verification). Flagship demo: trustless chess.
- Explorers: [explorer-tn10.kaspa.org](https://explorer-tn10.kaspa.org/), tn12.kaspa.stream (legacy). None decode covenant lineage — that's kascov's niche ([[Home]]).
