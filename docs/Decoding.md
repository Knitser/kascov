# Decoding

`kascov-decode` — turning covenant state scripts into something readable. Three layers, each additive: disassembly (always works) → spend-time reveals (the actual programs) → template recognition (named contracts with labeled fields).

## The fallback that always works

`DisasmDecoder` disassembles any script into named opcodes with the full post-Toccata opcode table (extracted from rusty-kaspa's `crypto/txscript/src/opcodes/mod.rs` at the pinned rev). Each instruction is tagged with a group: **push** · **standard** · **introspection** (KIP-17, 0xb2–0xc9 + `OpNum2Bin`/`OpBin2Num`) · **covenant** (KIP-20: `OpInputCovenantId`, `OpCovOutputIdx`, `OpOutputAuthorizingInput`, …) · **zk** (`OpZkPrecompile`, KIP-16).

`web/disasm.js` is a **verified port** of this disassembler — byte-identical output on every script in the live index — powering the site's [#/decode](https://kascov-explorer.web.app/#/decode) page, where any hex round-trips entirely in the browser.

## Spend-time decoding (shipped)

State UTXOs are usually **P2SH commitments** (`OpBlake2b <32B hash> OpEqual`): the program lives in the *spending* input's signature script. The [[Sync Engine]] captures every covenant spend's signature script (`spent_sig`, reorg-safe); `p2sh_reveal(spk, sig)` peels the final push (the redeem script) and accepts it **only if its blake2b-256 matches the committed hash**.

Where it surfaces:

- `show --decode` — prints the revealed program under each spent state UTXO
- `trace` — prints the revealed state **payload per event** and the **payload Δ** between consecutive reveals (the roadmap's original ask)
- exports/API — `revealed_hex`, `revealed_asm`, `revealed_uses_covenant_ops`/`_zk_ops`; non-P2SH spends carry `sig_hex` (≤520 B) or `sig_len`
- the web nerd panel — "revealed at spend" block with a decoder deep-link

Reveals exist for spends indexed from July 2 (evening) onward; earlier spends predate capture. Today's TN10 storm coins are P2PK-style (their unlocking script is just a signature — captured, shown as `spend signature …`); the reveal path lights up as P2SH covenants circulate.

## Template decoders (named contracts, labeled fields)

`Registry` tries `StateDecoder` implementations in order and falls back to disassembly. Registered:

| Decoder | Matches | Labeled fields |
|---|---|---|
| `TemplateDecoder` | compiled contracts by **skeleton matching** (constructor args are inlined at use sites) | per-template (below) |
| `P2pkStateDecoder` | `<push 32/33B> OpCheckSig` | `owner_pubkey` |
| `P2shCommitmentDecoder` | `OpBlake2b <32B> OpEqual` | `program_hash` |

Exports carry `template` + `state_fields` (and `revealed_template` + `revealed_fields` on reveals); the web nerd panel renders them as labeled rows, and named contracts get a pill on the coin page.

### SilverScript templates

Compiled [SilverScript](https://github.com/kaspanet/silverscript) contracts **inline constructor args at their use sites** (a pledge appears three times mid-script), so suffix/prefix splitting cannot work. Instead `Skeleton::derive(a, b, sentinels)` aligns **two builds of the same contract with distinct sentinel args** instruction-by-instruction: identical items become fixed ops/consts, differing pushes become labeled slots (looked up by sentinel value). Matching requires equal length, equal ops/consts, and *agreement across repeated slots* — the same arg pushed twice must carry the same value, which is what disambiguates Escrow from LastWill (identical arg shapes, different wiring).

Wired entries (derived at startup from six embedded `silverc` dumps, two per contract): **Mecenas** (`recipient`, `funder_hash`, `pledge`, `period`) · **Escrow** (`arbiter_hash`, `buyer`, `seller`) · **LastWill** (`inheritor_hash`, `cold_hash`, `hot_hash`). The same engine is ported to the browser (`web/disasm.js`), so the [decode page](https://kascov-explorer.web.app/decode) names pasted contracts too. Matching never guesses: no skeleton, no label.

Escrow and LastWill are indistinguishable by argument shape (both 3×32B), which is exactly why matching uses compiled bodies rather than arity heuristics.
