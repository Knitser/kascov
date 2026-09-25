# kascov-decode-wasm

[kascov-decode](../kascov-decode) as a WebAssembly module, for browsers and Node. It gives JavaScript the four answers of the `kascov-decode` command, and they are the same answers: the module and the command call the same code, and the tests check that they agree on every program fixture in the crate.

## Build

```bash
rustup target add wasm32-unknown-unknown
cargo build -p kascov-decode-wasm --target wasm32-unknown-unknown --release
```

The module lands at `target/wasm32-unknown-unknown/release/kascov_decode_wasm.wasm`. It is about 6 MB, nearly all of it the program bytes the decoder's skeletons are cut from, and about 290 KB gzipped. It imports nothing, so no generated glue is needed: [`js/kascov-decode.mjs`](js/kascov-decode.mjs) is the whole loader.

## Use

```js
import { load } from './kascov-decode.mjs';

// in a browser, fetch it
const kd = await load(new URL('./kascov_decode_wasm.wasm', import.meta.url));
// in Node, hand it the bytes
// const kd = await load(await readFile('kascov_decode_wasm.wasm'));

kd.name(programHex);              // { template, fields, generation, blake2b_256, ... }
kd.hash(programHex);              // { bytes, blake2b_256, p2sh_script }
kd.disasm(programHex);            // the opcode listing, as text
kd.check(signatureScriptHex, spentOutputScriptHex);  // { spk_matches, push_only, program, ... }
```

Every call takes hex text, with or without a `0x` prefix and line breaks. Input the decoder cannot read throws an `Error` that says why.

## Test

```bash
cargo build -p kascov-decode --release
KASCOV_DECODE_BIN=target/release/kascov-decode node --test crates/kascov-decode-wasm/js/kascov-decode.test.mjs
```

Without `KASCOV_DECODE_BIN` the comparison with the command is skipped and the rest still runs.
