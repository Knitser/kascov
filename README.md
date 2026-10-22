# kascov — the covenant explorer for Kaspa

**Live: [kascov.io](https://kascov.io)** · open JSON API, no keys · verify everything from chain

[![CI](https://github.com/Knitser/kascov/actions/workflows/ci.yml/badge.svg)](https://github.com/Knitser/kascov/actions/workflows/ci.yml)

## A note on the reset

If you followed this repository before: it used to carry the full kascov codebase, and one day it went quiet. Here is exactly what happened, because a project built on verification does not get to be vague about itself.

kascov outgrew its first shape. It began as an explorer that read covenants from the chain. Then it became a verifier that replays every trade against the covenant programs themselves. Then it became a trading terminal, where a market is spent directly against its on-chain program with no launchpad in the path. That growth deserved a rebuild of the foundations, and we took the codebase private to do that work properly rather than in public view, half-finished.

**The public explorer is coming back.** The explorer, the indexer and the open API belong in the open — that has not changed and will not. They return to this repository as the rework lands. What stays private is the trading machinery we operate as a product; what the chain proves remains checkable by anyone, from the chain, with or without us.

The first piece is back: [`crates/kascov-decode`](crates/kascov-decode), the decoder behind every name the explorer shows. See [below](#kascov-decode).

Nothing about the live site changed during any of this. [kascov.io](https://kascov.io) has been up the whole time, the API is open and free, and every verdict it publishes can still be re-derived from the same bytes we read.

## What kascov is

Kaspa's [Toccata hardfork](https://docs.kaspa.org/toccata) (June 30, 2026) let UTXOs carry rules. An output can be bound to a **covenant ID** ([KIP-20](https://github.com/kaspanet/kips/blob/master/kip-0020.md)) that survives being spent, so a coin has a *lineage*: genesis → every state transition → its current tip. Nodes validate that lineage; nothing exposes it. kascov reads it from the chain itself.

Every fact kascov publishes is derived from consensus data or from programs the chain revealed at spend time. There is no registry, no allowlist, no trusted submission path — if kascov claims a coin is a token, you can re-derive that claim from the same bytes.

## Live at kascov.io

- **[explore](https://kascov.io/explore)** — every smart coin with a friendly name, a life-story timeline, and the galaxy: every coin and the moves between them.
- **[tokens](https://kascov.io/tokens)** — the covenant-token directory with a deliberately conservative verdict: `verified` only when every event in a token's history matched a known rule and supply is conserved. Anything ambiguous stays `unvalidated` with a reason, never a false "valid".
- **[trade](https://kascov.io/kascov)** — buy and sell directly against a market's on-chain covenant. Your wallet signs only your own funding input; the page rebuilds the transaction from the curve's own formula and shows every output before anything is signed.
- **[playground](https://kascov.io/decode)** — paste any script hex and read it as opcodes, including the post-Toccata set. Runs in your browser.
- **[guide](https://kascov.io/guide)** — deploy your own covenant on testnet-10 in 15 minutes.
- **[API docs](https://kascov.io/#/dev)** — the whole JSON API, open, CORS `*`, no keys. If you build on it and our numbers ever disagree with yours, please open an issue: cross-implementation differences should surface as facts, not arguments.

## kascov-decode

The byte-level reader kascov.io uses to name covenant programs. Give it the bytes of a covenant program and it says what they are. It needs no node, no network and no database: it is plain functions over bytes, with five small dependencies (`hex`, `serde`, `serde_json`, `blake2b_simd`, `blake3`).

What it reads:

- **SilverScript contracts** from both compiler generations: the July 2026 fork (numeric dispatch selectors, BLAKE2b template hashes) and SilverScript 1.0 (four-byte KCC-1 dispatch tags, BLAKE3 template hashes). A match is made instruction by instruction against a skeleton cut from real compiler output, so it proves the template and returns the constructor values.
- **Argent apps** compiled by argentc in either generation, including the leader and delegate security checks argent added in September 2026, with each actor's template hash recomputed from its own bytes.
- **Launchpad and market builds live on Kaspa today**, from KRON, KaspaCom, KaspaKaha (now BraidSwap), KaspaRocket, Zealous and the gist-era KCC20 minter, each pinned to program bytes read off mainnet or testnet-10 that ship in `fixtures/`.
- **Token state** in the gist-era KCC20 cells that KRON, KaspaCom and KaspaKaha deploy, and in KCC-20 cells laid out as the spec defines them.
- **The KCC conventions at kaspanet/kccs 1ed87ae** (KCC-1, KCC-2 and KCC-20 Final, their text unchanged since 1516a6c; KCC-23 Draft), as library modules: the KCC-1 ABI codec, arrays and records included; KCC-2 authority schemes, with P2PKH keys hashed unkeyed as the Final text says and the keyed Draft hash kept for the programs compiled against it; KCC-20 transfers, borrows and issues checked against each build's pin; and KCC-23 metadata declarations verified against the covenant they bind. Each module is tested against the spec's own conformance vectors: see [Run the official vectors](#run-the-official-vectors). The `name` and `check` commands do not label KCC-20 builds yet: for them `template` is null.
- **Seven pinned KCC-20 builds** in [`crates/kascov-decode/pins/kcc0020/`](crates/kascov-decode/pins/kcc0020), each with a review note that says what was checked and how: five compiled against the Draft (kascov's own vector build and four ZealousSwap generations); the official reference build (argent-lang/kcc20-reference c8a0871, `9703112e`), reviewed against its reveals on mainnet and testnet-10; and KaspaRocket's token build (`ff3ae732`), the reference actor with its limits raised to 10 token inputs and 10 outputs, reviewed the same way. The reference pin also declares which never-spent genesis outputs of its tokens a reader may prove by rebuilding them byte for byte (`genesis_rebuild`).
- **vprogs** settlement and exit-permission programs, with what each settlement and exit claim states. It reads them and does not verify their proofs.
- **Anything else** as an opcode listing, including the post-Toccata covenant opcodes. This fallback always works.

### Use it

Install the command:

```bash
cargo install kascov-decode
```

or the latest from this repository with `cargo install --git https://github.com/Knitser/kascov kascov-decode`.

```text
kascov-decode name   <program hex | ->   what the program is, as JSON
kascov-decode disasm <program hex | ->   the opcode listing
kascov-decode hash   <program hex | ->   its BLAKE2b-256 and the P2SH script committing to it
kascov-decode check  <signature script hex | -> [--spk <script public key hex>]
```

`name` prints the verdict as JSON: the decoder that matched, the template, the compiler generation and the labelled fields, plus the Argent reading when there is one. The [crate README](crates/kascov-decode) has a worked example on real chain bytes. As a library:

```rust
let decoded = kascov_decode::Registry::default().decode(0, &program_bytes);
println!("{:?} {:?}", decoded.template, decoded.fields);
```

In a browser or Node, [`crates/kascov-decode-wasm`](crates/kascov-decode-wasm) builds the same answers into a WebAssembly module with a small JavaScript loader, and its tests check that the module and the command agree on every program fixture.

To build and test everything here:

```bash
cargo test --workspace
```

### Check kascov against the chain

Most program names kascov.io shows come from this crate, and those do not have to be taken on trust. A covenant output usually commits to its program as `aa20<blake2b-256 of the program>87`, and the input that spends it ends its signature script with a push of the full program. Take the spending transaction from your own Kaspa node and run

```bash
kascov-decode check <that input's signature script hex> --spk <the spent output's script public key hex>
```

It pulls out the program the spend revealed, checks that its BLAKE2b-256 matches what the output committed to, and names it. Compare the answer with kascov.io. If the two ever differ, please [open an issue](https://github.com/Knitser/kascov/issues).

What this recipe does not cover yet: builds that carry the spec KCC-20 state (the Draft-era ones included), and the market skeleton and Argent build labels kascov.io adds from its own index. kascov.io labels the KCC-20 builds from this crate's pins through the library (`kcc0020::Pin::family_label`, which falls back to `Pin::standing`), while the `name` and `check` commands do not label KCC-20 builds yet: for them `template` is null. That label names a build, never a token: a reviewed build reads "reviewed KCC-20 build", and whether one token of it is shown as a "KCC-20 token" is decided per token by kascov.io's derivation, which replays that token's history and is not in this repository yet. For each pinned build compiled by argentc, the same answer still carries `argent.template_hash` (under `program` in the `check` answer), and that hash names its pin's file in [`crates/kascov-decode/pins/kcc0020/`](crates/kascov-decode/pins/kcc0020), so the pin behind the label can be found by hand; an issuer's hash is listed under `issuers` in its token's pin file.

### Run the official vectors

```bash
cargo test -p kascov-decode --test kcc1_vectors --test kcc2_vectors --test kcc20_vectors --test kcc23_vectors -- --nocapture
```

These run the conformance files of kaspanet/kccs at 1ed87ae (byte-identical to those at 1516a6c, where they were copied), unmodified in [`crates/kascov-decode/tests/fixtures/kccs/`](crates/kascov-decode/tests/fixtures/kccs), through the crate's own functions: 103 checks for KCC-1 (one per vector), 35 for KCC-2 (5 constructions, 20 approval checks and 10 registry rows), 42 for KCC-20 and 24 for KCC-23 (9 on the binding, 3 on the mutations and 2 on each of the 6 declarations). The KCC-20 test counts 44, the 42 official checks plus 2 of its own harness, and the same run also reads Shawn Pearce's unofficial first-cut set (kccs PR #20) and kascov's own case for the one-class rule (#16), which the official set does not cover. Each runner asserts its count, so a vector that goes unread fails the run.

### Where the fixtures come from

Every program fixture was either read off Kaspa mainnet or testnet-10, or built from published source. [`PROVENANCE.md`](crates/kascov-decode/fixtures/PROVENANCE.md) says which, and for each program read off a chain it names the spend that revealed it, so the bytes can be checked from your own node. A test fails when a fixture is added without saying where it came from.

### Not here yet

The indexer, the API server and the web explorer are still being reworked in private, and return here as they land. The trading terminal stays private.

## Contact

[@kascovio](https://x.com/kascovio) · [kascov.io](https://kascov.io)

The code here is MIT licensed, as it was: see [LICENSE](LICENSE). Some test fixtures come from kaspanet/silverscript, argent-lang/argent and argent-lang/kcc20-reference under the ISC license, and the KCC conformance vectors from kaspanet/kccs under CC0-1.0: see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
