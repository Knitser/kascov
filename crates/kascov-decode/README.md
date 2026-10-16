# kascov-decode

Name Kaspa covenant programs from their bytes.

This is the decoder [kascov.io](https://kascov.io) uses to name covenant programs. A covenant output commits to its program by hash, and the spend that consumes it reveals the program as the last push of its signature script. Give `kascov-decode` those bytes and it says what the program is: which SilverScript or Argent build, which launchpad, market or token family, and what sits in its labelled fields. It needs no node, no network and no database.

## What it reads

- **SilverScript contracts** from both compiler generations: the July 2026 fork (numeric dispatch selectors, BLAKE2b template hashes) and SilverScript 1.0 (four-byte KCC-1 dispatch tags, BLAKE3 template hashes). A match is made instruction by instruction against a skeleton cut from real compiler output, so it proves the template and returns the constructor values.
- **Argent apps** compiled by argentc in either generation, including the leader and delegate security checks Argent added in September 2026, with each actor's template hash recomputed from its own bytes.
- **Launchpad and market builds live on Kaspa today**, from KRON, KaspaCom, KaspaKaha, Zealous and the gist-era KCC20 minter, each pinned to program bytes read off mainnet or testnet-10.
- **Token state** in the gist-era KCC20 cells that KRON, KaspaCom and KaspaKaha deploy, and in KCC-20 cells laid out as the spec defines them.
- **The KCC conventions at kaspanet/kccs 1516a6c** (KCC-1, KCC-2 and KCC-20 at Last Call, KCC-23 Draft), as library modules: the KCC-1 ABI codec, arrays and records included; KCC-2 authority schemes, with P2PKH keys hashed unkeyed as Last Call says and the keyed Draft hash kept for the programs compiled against it; KCC-20 transfers, borrows and issues checked against each build's pin; and KCC-23 metadata declarations verified against the covenant they bind. Each module is tested against the spec's own conformance vectors: see [Run the official vectors](#run-the-official-vectors). The `name` and `check` commands do not label KCC-20 builds yet: for them `template` is null.
- **vprogs** settlement and exit-permission programs, with what each settlement and exit claim states. It reads them and does not verify their proofs.
- **Anything else** as an opcode listing, including the post-Toccata covenant and ZK opcodes. This fallback always works.

## Install

```bash
cargo install kascov-decode
```

or straight from the repository:

```bash
cargo install --git https://github.com/Knitser/kascov kascov-decode
```

## Use

```text
kascov-decode name   <program hex | ->   what the program is, as JSON
kascov-decode disasm <program hex | ->   the opcode listing
kascov-decode hash   <program hex | ->   its BLAKE2b-256 and the P2SH script committing to it
kascov-decode check  <signature script hex | -> [--spk <script public key hex>]
```

`-` reads the hex from standard input, and the hex may carry a `0x` prefix and line breaks. Here is `name` on one of this crate's fixtures, a KaspaCom token program from testnet-10, run from `crates/kascov-decode` in a checkout:

```console
$ xxd -p fixtures/recovery/kcom_2671_parent_56fc521e_0.bin | kascov-decode name -
{
  "blake2b_256": "7cec521a9350d30408dae91466aeedc31fa0f34b43d30d6d23c06f650f333820",
  "bytes": 2671,
  "decoder": "template",
  "fields": [
    {
      "name": "owner_identifier",
      "value": "51fb56d74326e9f92d4866907994cb6442791f71f2a1a2fee0d408f1cdfa28d1"
    },
    {
      "name": "identifier_type",
      "value": "0000000000000000"
    },
    {
      "name": "amount",
      "value": "0040b824e75a0000"
    },
    {
      "name": "mint_mode",
      "value": "0200000000000000"
    },
    {
      "name": "mint_price_sompi",
      "value": "00e1f50500000000"
    },
    {
      "name": "treasury",
      "value": "51fb56d74326e9f92d4866907994cb6442791f71f2a1a2fee0d408f1cdfa28d1"
    },
    {
      "name": "ticker",
      "value": "5445535442450000000000000000000000000000000000000000000000000000"
    }
  ],
  "generation": null,
  "p2sh_script": "aa207cec521a9350d30408dae91466aeedc31fa0f34b43d30d6d23c06f650f33382087",
  "template": "KaspaCom · token",
  "uses_covenant_ops": true
}
```

`check` takes the signature script of an input that spent a covenant output, pulls out the program it revealed, and names it. Given the spent output's script public key with `--spk`, it also checks that the program hashes to what that output committed to, the same test kascov runs on every spend. It exits with 1 when the program does not match, and with 2 on input it cannot read.

## Check kascov against the chain

Most program names kascov.io shows come from this crate, and those do not have to be taken on trust:

1. Fetch the spending transaction from your own Kaspa node.
2. Run `kascov-decode check <that input's signature script> --spk <the spent output's script public key>`.
3. Compare the answer with kascov.io.

If the two ever differ, please [open an issue](https://github.com/Knitser/kascov/issues).

What this recipe does not cover yet: builds that carry the spec KCC-20 state (the Draft-era ones included), and the market skeleton and Argent build labels kascov.io adds from its own index. kascov.io labels the KCC-20 builds from this crate's pins through the library (`kcc0020::Pin::family_label`, which falls back to `Pin::standing`), while the `name` and `check` commands do not label KCC-20 builds yet: for them `template` is null. For each pinned build compiled by argentc, the same answer still carries `argent.template_hash` (under `program` in the `check` answer), and that hash names its pin's file in `pins/kcc0020/`, so the pin behind the label can be found by hand; an issuer's hash is listed under `issuers` in its token's pin file.

## Run the official vectors

From a checkout of the repository:

```bash
cargo test -p kascov-decode --test kcc1_vectors --test kcc2_vectors --test kcc20_vectors --test kcc23_vectors -- --nocapture
```

These run the conformance files of kaspanet/kccs at 1516a6c, copied unmodified into `tests/fixtures/kccs/`, through the crate's own functions: 103 checks for KCC-1 (one per vector), 35 for KCC-2 (5 constructions, 20 approval checks and 10 registry rows), 42 for KCC-20 and 24 for KCC-23 (9 on the binding, 3 on the mutations and 2 on each of the 6 declarations). The KCC-20 test counts 44, the 42 official checks plus 2 of its own harness, and the same run also reads Shawn Pearce's unofficial first-cut set (kccs PR #20) and kascov's own case for the one-class rule (#16), which the official set does not cover. Each runner asserts its count, so a vector that goes unread fails the run.

## As a library

```rust
use kascov_decode::{p2sh_reveal, Registry};

// the program a spend revealed, only if it hashes to the spent output's commitment
if let Some(program) = p2sh_reveal(&spent_output_script, &signature_script) {
    let decoded = Registry::default().decode(0, &program);
    println!("{:?} {:?}", decoded.template, decoded.fields);
}
```

`kascov_decode::report` gives the command's answers as JSON values.

## In the browser

[`crates/kascov-decode-wasm`](https://github.com/Knitser/kascov/tree/main/crates/kascov-decode-wasm) in the repository builds the same answers into a WebAssembly module, with a small JavaScript loader for browsers and Node.

## Where the fixtures come from

Every program fixture was either read off Kaspa mainnet or testnet-10, or built from published source. [`fixtures/PROVENANCE.md`](fixtures/PROVENANCE.md) says which, and for each program read off a chain it names the spend that revealed it, so the bytes can be checked from your own node. `tests/provenance.rs` fails when a fixture is added without saying where it came from.

## License

MIT, see [LICENSE](LICENSE). Some test fixtures come from kaspanet/silverscript, argent-lang/argent and argent-lang/kcc20-reference under the ISC license, and the KCC conformance vectors from kaspanet/kccs under CC0-1.0: see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
