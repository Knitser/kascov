# kascov — feature roadmap (next waves)

The "novel-features" wave (July 6, 2026) shipped 5 first-to-market things — covenant
simulation (CLI), verified contracts, a visual script debugger, based-app lanes, and a
KIP-16 ZK panel (see `Novel-Features-Plan.md`). This doc is the backlog *after* that:
what to deepen and what to build next, grouped by wave. Lift = S/M/L.

---

## Wave 1 — NEXT (in progress)

The picks that (a) make the flagship usable by everyone, (b) tie kascov to Kaspa's real
activity, and (c) add another nobody-has-it that's cheap because the foundation exists.

### 1. In-browser simulation on live coins  · lift M
Move covenant simulation out of the CLI (`kascov-lab --dry-run`) and into the web. On a
coin/decode page, construct a hypothetical spend (entrypoint, recipient, amount) → get
PASS/FAIL + the failing rule, in the browser.
- **Reuse:** the exact `simulate_input()` engine run already written in `kascov-lab`.
- **Approach:** worker `POST /simulate` endpoint (re-instantiate the contract with a
  stand-in signer so the full logic — sig + amount + destination + timelock — runs),
  returns `{pass, verdict, failing_rule}`. (WASM in-browser is the stretch alternative.)
- **Why:** rank 2 in the research; the natural home for the flagship.

### 2. KRC-20 / inscription decoding in lanes  · lift M
The biggest lane bucket is "JSON inscriptions" — decode them. Parse payloads into token
ops (deploy/mint/transfer, tick, amount) and give each namespace a per-app dashboard
(activity over time, top coins, holders).
- **Reuse:** payloads are already stored on `covenant_events`; lanes endpoint exists.
- **Why:** nobody ties KRC-20 to covenant lineage; turns a counts list into real analytics.

### 3. Covenant security / lint  · lift M
A static "audit" panel on any decoded covenant flagging risks: "anyone can spend this"
(no sig check), "no amount constraint", unbounded introspection, missing timelock, etc.
- **Reuse:** the disassembler + `web/vm.js` symbolic stepper.
- **Why:** no covenant linter exists anywhere — another genuine first, cheap to build.

---

## Wave 2 — deepen + visualize

### Real-spend debugger  · lift M · research rank 4
Today the debugger is a *symbolic* static trace. Extend it to replay an *actual* on-chain
spend: given a spend tx, run the real engine with the real witness and capture concrete
per-opcode stacks. Worker `/debug` driving `execute_opcode` + `stacks_view()`.

### App-graph / "follow the coin"  · lift M · research rank 5
Turn the existing covenant **families** (union-find) into an interactive force-graph:
nodes = covenants, edges = shared transactions, animated flow. A visual map of
multi-contract apps.

### Plain-English contract explainer  · lift S–M
Auto-generate a prose explanation for any decoded covenant (like the arbiter write-up),
built on the skeleton matcher.

### Time-series / analytics dashboards  · lift S
Births vs burns over time, per-template, per-lane, survival curves ("how long do
covenants live?"). Mostly aggregation + charts on top of the existing histogram.

---

## Wave 3 — bigger bets

### ZK: decode + actually verify the proof  · lift M (Groth16) / L (RISC Zero)
Beyond detecting `OpZkPrecompile`: parse proof / verification key / public inputs,
display them, and re-verify the Groth16 proof off-chain to show "proof valid ✓". Plus a
"ZK apps" filter on explore.

### Verify-and-publish  · lift M–L
Etherscan's flow: anyone submits SilverScript source + args → kascov compiles, checks
byte-identical, publishes as a verified contract. Grows the registry past the 3 templates.
Needs the `silverc` compiler reachable from the worker + a submission store.

### Covenant alerting & webhooks  · lift L · research rank 7
Subscribe to a covenant (coin moved/retired, escrow settled, ZK coin appeared) →
webhook/email/push. Reuses the SSE stream + events; the lift is the subscription store +
delivery.

### No-code covenant builder  · lift L · research rank 8
Beyond re-parameterizing the 3 templates: a guided visual builder (parties, conditions,
timelocks) that composes SilverScript and deploys.

---

## Also considered (parking lot)
- Address / entity labeling (Arkham-style) — data-thin for Kaspa today.
- MEV / ordering analysis on the BlockDAG — novel but data-thin + harder.
- Covenant state "diff" — what changed (program/args) between a coin's states.

**Process:** build a wave, verify everything end-to-end, then deploy. Keep local until
the wave is verified (same rhythm as the July-6 wave).
