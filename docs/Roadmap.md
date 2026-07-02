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
- **SilverScript template decoders** — recognize compiled contracts from `silverscript-lang/tests/examples/` (Mecenas, escrow, last-will) and name their state fields in the [[Decoding]] registry — surfaced on coin detail pages.
- **Snapshot sharding** — the TN10 full snapshot passed ~5 MB during the July 2 storm (2,900+ covenants); shard per-covenant or paginate before it hurts first paint.

## Publishing

- Repo: `github.com/Knitser/kascov` · dashboard: `kascov-explorer.web.app` (Firebase Hosting + Cloud Run worker).
- The pitch window is **open right now**: mainnet's first covenant appeared July 2 (~11:00 UTC, `c7948684ae…`, 195 events within the hour) and kascov indexed it live. TN10's same-day covenant storm (2,900+ covenants) shows the tester audience exists. Ship the "first covenant explorer for Kaspa L1" post immediately.

## Operational notes

- Bump the rusty-kaspa git pin **together with** the node version (borsh compatibility — [[Architecture#Design rules]] Rule 2).
- Testnet resets: `kascov reset --yes`, re-sync from scratch, done.
- An archival node would extend lineage beyond the ~3-day pruning window for covenants indexed late.
- Cloud Run worker: `scripts/deploy-worker.sh` (idempotent); DB continuity via `gs://kascov-explorer-index`; health probe is `/data/mainnet-live.json` (small forever) — `/healthz` is swallowed by the Google frontend on the deterministic URL.
