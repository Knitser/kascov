# kascov API clients

Tiny, zero-dependency wrappers over the [kascov JSON API](https://kascov.io/#/dev) — CORS-open, no keys.

- **`js/kascov.mjs`** — Node 18+ / browser, native fetch, SSE via async iterator.
- **`py/kascov.py`** — Python 3.9+, stdlib urllib only.

Both cover: live feed, paginated coin summaries, per-coin detail, tx/address
lookup, analytics, and the live SSE stream. They also expose the complete token
surface: token search and pagination; dedicated holders, events, and trades;
global trades; verified markets and graduated pools; commitment-proven vesting;
the machine index; and the OpenAPI 3.1 document.

```js
const page = await k.tokens({ status: 'verified', phase: 'bonding', limit: 50 });
const holders = await k.tokenHolders(page.tokens[0].covenant_id, { limit: 100 });
const markets = await k.markets({ priced: true });
const schedules = await k.vesting({ limit: 100 });
```

```py
page = k.tokens(status="verified", phase="bonding", limit=50)
holders = k.token_holders(page["tokens"][0]["covenant_id"], limit=100)
markets = k.markets(priced=True)
schedules = k.vesting(limit=100)
```

All amounts remain exact JSON integers. Market prices remain exact
`quote_sompi / base_amount` pairs, and vesting coordinates are DAA scores rather
than timestamps.

The stream is `GET /data/{network}/stream` — the same route the worker registers
and the site itself uses. An optional per-covenant filter narrows it to one coin:

```js
for await (const ev of k.stream({ covenant: id })) ...   // js
```
```py
for ev in k.stream(covenant=cid): ...                    # py
```

The filter must be exactly 64 hex chars; anything else is a `400` from the
server, never a silent firehose.

Neither client needs a token. An optional lane token — minted at
[kascov.io/lane](https://kascov.io/lane) — is sent as an `X-Kascov-Lane` header
on every request when configured. It buys extra request capacity on the holder
lane and nothing else: no influence on verdicts, and the anonymous tier keeps
working without it.

```js
const k = new Kascov('mainnet', 'https://kascov.io', { laneToken: '...' });
```
```py
k = Kascov("mainnet", lane_token="...")
```

Tests pin the exact URL shapes against the routes in `crates/kascov/src/main.rs`:

    node --test clients/js/kascov.test.mjs
    python3 clients/py/test_kascov.py

They live in-repo so versioning tracks the API. Publishing to npm / PyPI is a
deliberate separate step — copy the single file into your project meanwhile.
