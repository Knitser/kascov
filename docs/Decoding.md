# Decoding

`kascov-decode` — turning covenant state scripts into something readable.

## The fallback that always works

`DisasmDecoder` disassembles any script into named opcodes with the full post-Toccata opcode table (extracted from rusty-kaspa's `crypto/txscript/src/opcodes/mod.rs` at the pinned rev). Each instruction is tagged with a group:

- **push** — data pushes (the candidates for state fields)
- **standard** — pre-Toccata opcodes
- **introspection** — KIP-17 transaction introspection (0xb2–0xc9, `OpNum2Bin`/`OpBin2Num`)
- **covenant** — KIP-20 ops (`OpInputCovenantId`, `OpCovOutputIdx`, `OpOutputAuthorizingInput`, …)
- **zk** — `OpZkPrecompile` (KIP-16)

`show --decode` output from a real TN10 covenant:

```
State  1a9487f5…f674d5:0 — 1.00000000 KAS (spk v0, 35 bytes)
  0000  OpBlake2b
  0001  OpData 0xc5608c8a1186226b7822a7485effadbaeca493f90e7f1803404d32b6af90cf8a
  0022  OpEqual
```

That's a **P2SH commitment** — consistent with [[Toccata Protocol Notes#KIP-20 mechanics (what kascov indexes)]]: the state UTXO commits to a script hash; the actual covenant logic (the preimage) is revealed in the *spending* transaction's signature script.

## Decoder registry

`Registry` tries template-specific `StateDecoder` implementations in order and falls back to disassembly. Decoders are additive — recognizing a known SilverScript template means naming its state fields from the ordered pushes. None registered yet; the registry exists so adding one never touches the CLI.

## Spend-time decoding (next)

Because logic lives in the P2SH preimage, the deep decode target is the **spending input's signature script**, not the output. Plan ([[Roadmap]]): capture signature scripts of covenant-spending transactions during [[Sync Engine|sync]], disassemble the revealed preimage, and diff state pushes across transitions (`payload Δ` in `trace`).
