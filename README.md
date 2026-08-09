# kascov — the covenant explorer for Kaspa

**Live: [kascov.io](https://kascov.io)** · open JSON API, no keys · verify everything from chain

## A note on the reset

If you followed this repository before: it used to carry the full kascov codebase, and one day it went quiet. Here is exactly what happened, because a project built on verification does not get to be vague about itself.

kascov outgrew its first shape. It began as an explorer that read covenants from the chain. Then it became a verifier that replays every trade against the covenant programs themselves. Then it became a trading terminal, where a market is spent directly against its on-chain program with no launchpad in the path. That growth deserved a rebuild of the foundations, and we took the codebase private to do that work properly rather than in public view, half-finished.

**The public explorer is coming back.** The explorer, the indexer and the open API belong in the open — that has not changed and will not. They return to this repository as the rework lands. What stays private is the trading machinery we operate as a product; what the chain proves remains checkable by anyone, from the chain, with or without us.

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

## Contact

[@0xKnitser](https://x.com/0xKnitser) · [kascov.io](https://kascov.io)

The code that returns here will be MIT, as it was.
