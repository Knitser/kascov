# Roadmap

Rough priority order, July 2026. Updated July 2 after the realtime/explorer wave shipped.

## Shipped (July 2, 2026)

- ~~**kascov-api**~~ — landed as `kascov serve` (axum in the CLI crate, not a separate crate): an always-on Cloud Run worker follows the chain per network and serves `/data/<network>.json` (full snapshot) + `/data/<network>-live.json` (stats + tip + newest ~150 events), CORS `*`, GCS-backed SQLite restore/backup. Per-covenant endpoints + SSE remain open (below).
- ~~**Web UI**~~ — went past "static covenant browser": live-sync badge with exact tip-anchored times (UTC on hover), 12s light poller, paste-a-txid search that jumps to the coin and flashes the event, watchlist, record holders, sorting, `#/decode` (in-browser post-Toccata disassembler, byte-identical to kascov-decode), `#/dev` (API docs + field dictionary + quickstarts).
- **Sync catch-up throughput** — accepting blocks are now prefetched concurrently (ordered, `FETCH_AHEAD=16`); sequential WAN fetches couldn't outrun TN10's 10 bps during the July 2 covenant storm. Store gained a 10s busy timeout so backups stop failing silently under write load.
- **Ops** — production data no longer depends on the laptop: Firebase Hosting serves the static site; `/data/**` rewrites to the Cloud Run worker; `scripts/kascov-live.sh` is obsolete as a publisher (still fine as a local dev follower).

## Near

- ~~**Spend-time decoding**~~ — **shipped July 2 (evening)**: sync captures every covenant spend's signature script (`spent_sig`, additive migration, reorg-safe); `kascov_decode::p2sh_reveal` verifies + peels the redeem script against the committed blake2b hash; `trace` prints per-event state payloads and the `payload Δ` between reveals; exports carry `revealed_hex`/`revealed_asm` (+ op flags) and the web nerd panel shows "revealed at spend" with a decoder deep-link. Reveals exist for spends indexed from this version on.
- ~~**KIP-20 genesis hash validation**~~ — **shipped July 2 (evening)**: classification recomputes the id via the consensus `covenant_id` from the pinned rusty-kaspa rev (`node::compute_covenant_id` boundary wrapper); unprovable first sightings are recorded as transitions with `lineage_complete = false`.
- **Notification-based follow** — subscribe to `virtual-chain-changed` instead of the 2s poll in `sync --follow`. Deliberately deferred: with concurrent prefetch the end-to-end freshness is already ~2–4s, so notifications buy ~1–2s at the cost of new reconnect/subscription failure modes. Do it when SSE push lands (same plumbing).

## Mid

- **Per-covenant API + SSE** — `/covenants/:id` JSON and an `/events` push stream on the worker; the web app's poller can then switch to push.
- ~~**SilverScript template decoders**~~ — **shipped July 2 (late)**: data-driven `TemplateDecoder` (invariant-body suffix match, labeled constructor pushes), `Template::derive_body` for extraction, p2pk/p2sh recognizers live; Mecenas/Escrow/LastWill entries wired, bodies pending one compile run of silverscript-lang ([[Decoding#SilverScript templates]]).
- **Snapshot sharding** — the TN10 full snapshot passed ~10 MB during the July 2 storm (6,000+ covenants); shard per-covenant or paginate before it hurts first paint. Brotli (shipped) buys time.

## Toccata coverage gaps (audit July 2, vs docs.kaspa.org/toccata)

What kascov covers: covenant ids/lineage (consensus-validated genesis), P2SH reveals, introspection/covenant/zk op visibility, template recognition. What Toccata offers that kascov doesn't surface yet, in suggested order:

1. **Transaction payloads** — v1 txs carry payloads (`OpTxPayload*` introspection exists); capture payload bytes of covenant-touching txs at sync (same touchpoint as sig capture) and show them on the timeline. Covenant apps will stash state/messages there.
2. **Committed compute budgets** — v1 inputs carry compute-budget commitments; capture per covenant spend and display ("this transition budgeted N units"). Same sync touchpoint.
3. **ZK precompile labeling** — when `OpZkPrecompile` covenants appear, decode which system (Groth16 vs RISC Zero) from the invocation shape; a decoder heuristic, cheap.
4. **User lanes + gas commitments** — non-native subnetwork lanes with gas admission; kascov ignores `subnetwork_id`/`gas` entirely. A "lanes" activity view is the based-apps counterpart of the covenant explorer.
5. **Seqcommit lanes (KIP-21)** — `OpChainblockSeqCommit` is flagged in scripts but lane commitments aren't indexed; pairs with vprogs-based apps maturing.
6. **Auth/covenant group visualization** — multi-covenant transactions: which inputs authorize which outputs, as a small diagram on the coin page (data already indexed).

## Publishing

- Repo: `github.com/Knitser/kascov` · dashboard: `kascov-explorer.web.app` (Firebase Hosting + Cloud Run worker).
- The pitch window is **open right now**: mainnet's first covenant appeared July 2 (~11:00 UTC, `c7948684ae…`, 195 events within the hour) and kascov indexed it live. TN10's same-day covenant storm (2,900+ covenants) shows the tester audience exists. Ship the "first covenant explorer for Kaspa L1" post immediately.

## Operational notes

- Bump the rusty-kaspa git pin **together with** the node version (borsh compatibility — [[Architecture#Design rules]] Rule 2).
- Testnet resets: `kascov reset --yes`, re-sync from scratch, done.
- An archival node would extend lineage beyond the ~3-day pruning window for covenants indexed late.
- Cloud Run worker: `scripts/deploy-worker.sh` (idempotent); DB continuity via `gs://kascov-explorer-index`; health probe is `/data/mainnet-live.json` (small forever) — `/healthz` is swallowed by the Google frontend on the deterministic URL.
