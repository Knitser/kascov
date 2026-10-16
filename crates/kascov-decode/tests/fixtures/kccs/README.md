# Official KCC conformance vectors

Copied unmodified from kaspanet/kccs at 1516a6c (Oct 14 2026): KCC-1 and
KCC-2 in Last Call, KCC-20 in Last Call, KCC-23 Draft. Refresh by copying
the `vectors/` folders again; the test runners read these files as they are.

`kcc-0020/pearce-pr20-vectors.json` is not part of the official set. It is
Shawn Pearce's first cut of KCC-20 conformance vectors, copied unmodified
from `kcc-0020/vectors/vectors.json` at 81f111f0 on ShawnPearce/kccs, the
head of kaspanet/kccs PR #20 ("KCC-0020: first-cut conformance vectors",
closed 2026-09-27). Its reject cases were reworked into the official
format as saefstroem/kccs PR #1 (merged 2026-09-27 into the Last Call
branch, merge cee74605): the six `standard_transfer` rejects plus
`successor-changes-owner`, `successor-reduces-kas-value` and
`hash-chain-link-mismatch` in `kcc-0020/conformance.json`. kascov runs both
files (tests/kcc20_vectors.rs).
