# Third-party notices

The kascov code in this repository is MIT licensed (see [LICENSE](LICENSE)). Some test fixtures come from other projects, under their own licenses:

- `crates/kascov-decode/tests/fixtures/silverscript/` holds example contracts from [kaspanet/silverscript](https://github.com/kaspanet/silverscript) and its fork michaelsutton/silverscript, and the SilverScript dumps in `crates/kascov-decode/src/lib.rs` are those compilers' output for them.
- `crates/kascov-decode/fixtures/argent/` holds artifacts written by argentc from [argent-lang/argent](https://github.com/argent-lang/argent), copied byte for byte (see that folder's README).
- `crates/kascov-decode/fixtures/kcc0020/reference_*.bin` and `synthetic_reference_*.bin` are the compiled bytecode of the official KCC-20 reference app, [argent-lang/kcc20-reference](https://github.com/argent-lang/kcc20-reference) at c8a0871 (`fixtures/public-mint/artifact.json`): the `reference_*` files as revealed on mainnet and testnet-10, the `synthetic_reference_*` files with their state windows filled in by hand; `crates/kascov-decode/pins/kcc0020/9703112e.json` describes that build.
- `crates/kascov-decode/tests/fixtures/kccs/` holds the conformance vectors of [kaspanet/kccs](https://github.com/kaspanet/kccs), copied unmodified, and one vector file from its fork ShawnPearce/kccs (see that folder's README).

Program bytes in `crates/kascov-decode/fixtures/` otherwise come from the public Kaspa mainnet and testnet-10 chains, or were built from published source: [PROVENANCE.md](crates/kascov-decode/fixtures/PROVENANCE.md) says which.

## kaspanet/silverscript

ISC License

Copyright 2026 Kaspa Developers

Permission to use, copy, modify, and/or distribute this software for any purpose with or without fee is hereby granted, provided that the above copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

## argent-lang/argent

ISC License

Copyright 2026 Argent-lang developers

Permission to use, copy, modify, and/or distribute this software for any purpose with or without fee is hereby granted, provided that the above copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

## argent-lang/kcc20-reference

ISC License, as its Cargo.toml declares (`license = "ISC"`). The repository carries no LICENSE file and no copyright line, so the permission notice is reproduced without one.

Permission to use, copy, modify, and/or distribute this software for any purpose with or without fee is hereby granted, provided that the above copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

## kaspanet/kccs

CC0 1.0 Universal (SPDX-License-Identifier: CC0-1.0). Every document in that repository, including its auxiliary material, is dedicated to the public domain under CC0 1.0 unless it states otherwise; the fork ShawnPearce/kccs carries the same LICENSE.md. Legal code: <https://creativecommons.org/publicdomain/zero/1.0/legalcode>.
