# kascov — five first-to-market features (build plan)

Goal: build all five, **local only — no git push, no deploy** until every one is verified end to end. Order: simulation → verified registry → debugger → KIP-21 lanes → KIP-16 ZK. Research (deep-research pass) folds into the two KIP features.

Feasibility confirmed against the pinned rusty-kaspa + our crates:
- `kaspa_txscript::TxScriptEngine` — `execute()` (pass/fail + `TxScriptError`), `execute_and_return_stacks()`, `stacks_view()` (public), `opcode_execution_log_buffer`. `kascov-lab` already links `kaspa-txscript`.
- `kascov-decode` already tags `OpZkPrecompile` (0xa6) as the `Zk` group; `sync.rs::classify` already sees `tx.payload` + `compute_budget`.
- The worker (`kascov`) links `kascov-core` + `kascov-decode`; it can gain `kaspa-txscript` for /simulate + /debug endpoints.

## 1. Covenant simulation — "what-if spend" (Tenderly for covenants)
Dry-run a spend through the real script engine without broadcasting; report PASS / FAIL + the exact failing rule + script-units used.
- **kascov-lab `simulate`**: refactor `spend`/`settle_escrow` into `build_spend(...) -> (MutableTransaction, entries, covenant_id)`; `spend` submits, `simulate` runs `TxScriptEngine::from_transaction_input(...).execute()` and prints the verdict. No key-match bail — simulate even a spend you can't sign (report the checksig failure honestly).
- **Web** (later, needs worker): a "test a spend" panel on the coin page hitting a worker `/simulate` endpoint.
- Unique: nobody simulates covenant spends on Kaspa; the engine is already wired.

## 2. Verified covenant registry (Etherscan "verified source", for covenants)
Publish SilverScript source for a program hash and prove it by BLAKE2b-matching the compiled bytes; coins with that hash show a ✓ verified badge + readable source.
- v1 curated: `web/verified.json` mapping `program_hash -> {name, author, source}`, seeded with the 3 canonical contracts (their real .sil). Frontend: any coin whose committed/revealed `program_hash` is in the registry shows "✓ verified source" with a source viewer.
- v2 (optional): worker `POST /verify {program_hex, source}` → confirm `blake2b(program)` is a real on-chain hash → store; guarded against spam.
- Unique: no "verified contract" concept exists for Kaspa covenants.

## 3. Visual script debugger (step the stack, opcode by opcode)
Step through a program's execution against a real spend, showing the data/alt stacks after each opcode.
- **Backend**: worker `/debug` (or a `kascov-lab debug`) runs the engine capturing per-opcode stacks (drive `execute_opcode` + `stacks_view()`, or parse the `opcode_execution_log_buffer`), returns `[{pc, opcode, dstack, astack, err?}]`.
- **Web**: a scrubber on the decode/coin page — disassembly with the current op highlighted + the live stack panel.
- Unique: no covenant/script step-debugger exists; the engine can be stepped.

## 4. KIP-21 based-app lane analytics
Decode v1 payload lanes (namespace + gas) on events; aggregate "based-app activity" per namespace.
- **Indexer**: in `classify`/event write, parse a KIP-21 lane payload → store `lane_namespace`. **Endpoint** `/data/{net}/lanes.json`. **UI**: a "based-app lanes" section (auto-hides when empty, like families).
- Data-blocked today (zero lane payloads seen) — build detection + endpoint + UI so it lights up when traffic starts. Exact payload format from Toccata/KIP-21 docs (research).
- Unique: confirmed nobody exposes lane analytics.

## 5. KIP-16 ZK-app explorer
Flag and inspect covenants whose program uses on-chain Groth16 verification (`OpZkPrecompile`).
- **Detect**: program (revealed, or committed if a bare script) contains `0xa6`. Mark the coin/template "ZK".
- **UI**: a ZK badge + a panel explaining the verified proof, a "zk apps" filter/section, and (if feasible) surfacing the verifying-key/public-input pushes around the opcode.
- Unique: on-chain ZK verification is brand new; no explorer inspects ZK covenants.

## Verification (all local)
- Rust: `cargo test --workspace`; run `kascov-lab simulate`/`debug` on real TN10 coins (PASS + FAIL cases).
- Worker: run `kascov serve` locally, curl the new endpoints; point the web preview's data proxy at the local worker to exercise /simulate + /debug end to end.
- Web: `node --check`, CDP smoke of each new UI on the localhost preview.
- Nothing is pushed or deployed until the whole set passes.
