# Toccata Protocol Notes

The **Toccata** hardfork made Kaspa L1 programmable. Announced by [@kaspaunchained](https://x.com/kaspaunchained/status/2071999597036101937): *"Toccata is live! $KAS became ultra programmable today!"* Reference: [docs.kaspa.org/toccata](https://docs.kaspa.org/toccata).

## What it bundles

| KIP | What it adds |
|---|---|
| KIP-17 | Extended script opcodes — transaction introspection (0xb2–0xc9), the covenants backbone |
| KIP-20 | **Covenant IDs** — consensus-level lineage tracking for stateful UTXOs |
| KIP-16 | ZK opcodes — Groth16 + RISC Zero verification via `OpZkPrecompile` |
| KIP-21 | Partitioned sequencing commitments for based ZK apps (`OpChainblockSeqCommit`) |
| Tx v1 | `TX_VERSION_TOCCATA = 1`: covenant output bindings, input compute budgets, gas commitments |

## KIP-20 mechanics (what kascov indexes)

- A transaction output may carry a `CovenantBinding { authorizing_input: u16, covenant_id: Hash }`.
- **Genesis:** a new covenant id = BLAKE2b-256 (domain tag `"CovenantID"`) over the authorizing input's outpoint + the ordered list of authorized outputs (index, value, script pubkey — *excluding* the binding itself). Implementation: `consensus/core/src/hashing/covenant_id.rs`; both the [[Covenant Lab]] and the [[Sync Engine#Classification|classifier's genesis validation]] call this helper directly.
- **Continuation:** an output's covenant id must match the covenant id of the UTXO spent by its authorizing input.
- **Burn:** a covenant UTXO is spent with no successor output claiming the id.
- Spent covenant UTXOs are visible in RPC as `UtxoEntry.covenant_id: Option<Hash>`.
- State lives **in the script itself** — two shapes observed live: plain P2PK states (`<pubkey> OpCheckSig`, the TN10 storm cohort) and P2SH commitments (`OpBlake2b <32B hash> OpEqual`) whose program is revealed at spend time in the signature script. kascov captures and verifies those reveals — see [[Decoding#Spend-time decoding (shipped)]].

## Networks

| Network | Toccata | Notes |
|---|---|---|
| mainnet | ✅ active since DAA 474,165,565 (June 30, 2026) | **first covenants arrived July 2, ~11:00 UTC** (`c7948684ae…`, 195 events in its first hour; 5 covenants / 855 events by evening) — kascov indexed them live |
| testnet-10 | ✅ active since DAA 467,579,632 | **heavy, bursty covenant traffic**: the July 2 storm took the index from ~1,100 to 5,800+ covenants (24k+ events) within hours |
| testnet-12 | ⚠️ legacy | the pre-fork covenant playground; runs a separate `tn12` node branch with `ForkActivation::never()` on mainnet params. SilverScript's "TN12 only" README note is stale. |

Node: rusty-kaspa **v2.0.1** (mainnet + TN10). Default borsh wRPC ports: mainnet `17110`, testnet `17210`. Public resolver serves mainnet + TN10, **not** TN12.

## Ecosystem

- **SilverScript** ([kaspanet/silverscript](https://github.com/kaspanet/silverscript)) — the covenant language; compiles to native Kaspa Script. Its example contracts (Mecenas, Escrow, LastWill) are wired into kascov's [[Decoding#Template decoders (named contracts, labeled fields)|template registry]].
- **vprogs** ([kaspanet/vprogs](https://github.com/kaspanet/vprogs)) — Rust framework for based ZK apps (off-chain compute, on-chain RISC Zero proof verification). Flagship demo: trustless chess.
- Explorers: [kascov-explorer.web.app](https://kascov-explorer.web.app) (covenant lineage — this project), [tn10.kaspa.stream](https://tn10.kaspa.stream/) (general blocks/txs/addresses), explorer-tn10.kaspa.org. kascov remains the only one decoding covenant lineage.
