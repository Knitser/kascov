# kascov roadmap v2 — after the launch wave

*Written July 4, 2026 — two days after mainnet's first smart coins, one day after launch (325 likes, 6.8k views, real traffic). TN10 at 42k coins / 191k events and accelerating.*

## Where we stand

Shipped and proven: acceptance-driven indexer (reorg-safe, self-healing), KIP-20 genesis validation, spend-time reveals, SilverScript recognition (server + browser), the explorer UI (live feed, life stories, watchlist, records, search+suggest), in-browser decoder with example gallery, JSON API, GCS-backed durability, and — today — the scale rework (grid/detail split, single-flight builds, pre-compressed responses, CDN absorption) that survived the first traffic spike and a 40× data explosion.

The next wave sorts by one question: **what makes people come back tomorrow?**

## Tier 1 — retention features (do these first)

### 1. Address pages
"Which smart coins has address X touched?" is the most-asked explorer question in existence and nobody can answer it for covenants. We already store every UTXO's script; deriving the controlling address for p2pk-shaped states is one function away. Route: `#/addr/<kaspa:…>` — every covenant an address funded, received from, or controls. **This makes kascov a daily tool for anyone testing their own contracts.**

### 2. Live event stream on the site (SSE)
The 12s poll is fine; a `/data/{net}/stream` Server-Sent-Events endpoint that pushes births/moves/burns the second they're accepted turns the explorer into a *screen people leave open*. The worker already sees events in real time (`follow_forever`); fanning them out to N SSE clients is cheap. Falls back to polling automatically.

### 3. Contract-type analytics
We recognize Mecenas/Escrow/LastWill + p2pk/p2sh shapes. Aggregate it: "what's running on this network" — counts by template, adoption over time, a pie that answers "is anyone actually using escrows?" Nobody else can even see this. One SQL GROUP BY + one chart.

### 4. Daily digest endpoint (+ optional bot)
`/data/{net}/digest.json`: births/deaths/moves/value-moved in the last 24h, biggest coin, busiest coin. Powers a "today on Kaspa smart coins" card on the landing page — and the same JSON can feed a Telegram/X bot later (the community *loves* daily-stats bots; free recurring distribution).

## Tier 2 — depth for builders

### 5. Covenant families (lineage graphs)
Covenants that share a genesis tx or spend into each other form applications ("complex stateful multi-contract flows" — the Toccata docs' own words). Cluster them, draw the family tree on the coin page. This is the feature that makes app developers send *their users* to kascov.

### 6. Payload decoding
We already store payloads (based-app data). Add hex+ASCII+JSON auto-detection in the UI, and template-aware field labeling where SilverScript metadata exists. The "what did this app write on-chain" view.

### 7. Compute-budget analytics
We capture `spent_budget` per spend. Chart budget usage by template/coin — "how expensive is a Mecenas claim in practice?" Real fee-planning data for contract authors; literally nobody else has it.

### 8. Webhooks for watched coins
A tiny `POST /api/watch {covenant_id, url}` — the worker POSTs when the coin moves. Builders wire their covenant into CI/monitoring in one curl. (Needs light abuse protection: per-IP caps, retry budget.)

## Tier 3 — reach

### 9. OG cards per coin
Server-render a share image (name, avatar, status, balance, template pill) for `/mainnet/c/<id>` links. Every tweet about a covenant becomes a kascov billboard. The worker can rasterize the same deterministic avatar SVG.

### 10. Embeddable widget
`<script src="…/embed.js" data-covenant="…">` → a live coin card for docs/blogs. SilverScript tutorials would embed these on day one.

### 11. `kascov-py` / `kascov-js` thin clients
The API is stable now; 50-line typed wrappers make hackathon adoption instant.

## Ops guardrails (quiet, ongoing)

- **Alert policy** on the uptime check (one gcloud command away once the email channel is attached).
- **Archive tier**: nightly DB copy to a second bucket with 90-day retention — belt over the 5-min braces.
- **Load test before the next viral moment**: k6 script hitting grid+detail at 100 rps, so the next spike is boring.
- **Testnet resets**: TN10 resets happen; the self-recovery already handles the cursor — add a "network was reset, history preserved from before the reset" banner when detected.

## Explicit non-goals (for now)

- General block/address explorer (tn10.kaspa.stream does it well — we link out).
- Accounts/logins — watchlist stays local; webhooks are keyless with caps.
- Historical *price* anything.

## Suggested order

Address pages → contract-type analytics → SSE stream → digest (+bot) → OG cards → families → payloads → budgets → webhooks → widget → clients.

Address pages and analytics are each roughly a day; SSE+digest another; then reassess by what users actually click (the digest bot doubles as a growth loop). Ship one visible thing per day while the launch attention is warm.
