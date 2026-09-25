# kascov-decode

Name Kaspa covenant programs from their bytes.

This is the decoder behind every name [kascov.io](https://kascov.io) shows. A covenant output commits to its program by hash, and the spend that consumes it reveals the program as the last push of its signature script. Give `kascov-decode` those bytes and it says what the program is: which SilverScript or Argent build, which launchpad, market or token family, and what sits in its labelled fields. It needs no node, no network and no database.

## What it reads

- **SilverScript contracts** from both compiler generations: the July 2026 fork (numeric dispatch selectors, BLAKE2b template hashes) and SilverScript 1.0 (four-byte KCC-1 dispatch tags, BLAKE3 template hashes). A match is made instruction by instruction against a skeleton cut from real compiler output, so it proves the template and returns the constructor values.
- **Argent apps** compiled by argentc in either generation, including the leader and delegate security checks Argent added in September 2026, with each actor's template hash recomputed from its own bytes.
- **Launchpad and market builds live on Kaspa today**, from KRON, KaspaCom, KaspaKaha, Zealous and the KCC-20 minter, each pinned to program bytes read off mainnet or testnet-10.
- **Token state** in KCC-20 and KCC-0020 cells.
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

Every name kascov.io shows comes from this crate, so none of them has to be taken on trust:

1. Fetch the spending transaction from your own Kaspa node.
2. Run `kascov-decode check <that input's signature script> --spk <the spent output's script public key>`.
3. Compare the answer with kascov.io.

If the two ever differ, please [open an issue](https://github.com/Knitser/kascov/issues).

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

MIT, see [LICENSE](LICENSE). Some test fixtures come from kaspanet/silverscript and argent-lang/argent under the ISC license: see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
