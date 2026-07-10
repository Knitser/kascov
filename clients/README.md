# kascov API clients

Tiny, zero-dependency wrappers over the [kascov JSON API](https://kascov.io/#/dev) — CORS-open, no keys.

- **`js/kascov.mjs`** — Node 18+ / browser, native fetch, SSE via async iterator.
- **`py/kascov.py`** — Python 3.9+, stdlib urllib only.

Both cover: live feed, paginated coin summaries (compound cursor), per-coin detail
(events/UTXOs/holders), tx & address lookup, digest, galaxy, reorgs, templates,
activity, and the live SSE stream.

They live in-repo so versioning tracks the API. Publishing to npm / PyPI is a
deliberate separate step — copy the single file into your project meanwhile.
