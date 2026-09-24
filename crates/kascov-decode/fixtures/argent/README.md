# Argent artifact fixtures

Two generations of the `artifact.json` argentc writes, used as test fixtures
for the artifact intake and the argent recognizer. Argent is ISC licensed,
Copyright 2026 Argent-lang developers. No app here was ever deployed on a
Kaspa network; the fixtures name templates, not covenants.

## Fork era (argentc 0.1.0 at 05ba4b2, silverscript fork)

`tickets.artifact.json` and `icc_kcc20_asset.artifact.json` are the files
argentc wrote for two of the examples in https://github.com/argent-lang/argent
(`examples/tickets.ag` and `examples/icc/kcc20_asset.ag`), copied byte for
byte. Old shape: `sil_abi.contracts` is an array, each contract carries
`compiled.script_hex`, `compiled.template_hash_hex` and `compiled.state_span`,
and entries carry a `selector`. Every contract is a guardless leaf whose
`template_hash_hex` must reproduce from its own `script_hex` and `state_span`
under the BLAKE2b template rule the fork-era programs check in-script.

## SilverScript 1.0 (argentc at 867b080, pins kaspanet/silverscript v1.0.0)

`tickets.v1.artifact.json` and `icc_kcc20_asset.v1.artifact.json` are the
same two examples built by the current argentc, copied byte for byte from
`examples/build/*/artifact.json`. `entry_range_outputs.v1.artifact.json` is
argent's own `tests/fixtures/emit/entry_range_outputs/artifact.json`, the
Batch actor emitting `Account[1..=MAX_ACCOUNTS]` behind two fixed handles.
`entry_range_only.v1.artifact.json` is the same app with the fixed handles
removed (source in `entry_range_only.ag`), compiled by kascov through that
argentc so the recognizer's no-fixed-handle range form is proven on the real
compiler's bytes rather than read off its source.

`open_icc_core.v1.artifact.json` and `stones.v1.artifact.json` are argent's
`examples/build/*/artifact.json`, and `entry_range_inputs.v1.artifact.json` its
`tests/fixtures/emit/entry_range_inputs/artifact.json`, all at `e76ee07`, the
commit that implements the leader/delegate security rules 5 and 6. Their
coordinated leader entries that consume inputs carry rule 5's
`require(OpCovOutputCount(cov) == OpAuthOutputCount(active))`, compiled to
`d2 b9 cb 9c 69`, which the recognizer reads beside the count site it restates.

1.0 shape: `sil_abi.contracts` and each contract's `entries` are maps keyed
by name, `compiled.bytecode` and `compiled.template_hash` are byte arrays,
entries carry a `dispatch_tag`, and `sil_abi` carries `structs` and
`compiler_version`. Every contract guards its state with `6b … 6c` and
dispatches on `OpDup OP_DATA_4 <tag> OpEqual OpIf OpDrop` rungs; its
`template_hash` must reproduce under the BLAKE3 rule (`kcc1::template_hash`)
that a 1.0 program checks with `OpBlake3`. `schema_version` stays 1 across
both shapes; the intake tells them apart by `compiled.bytecode`.
