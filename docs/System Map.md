# System Map

This is the high-level map of kascov: where data enters, which boundary owns each transformation, what persists, and what users consume.

## End-to-end data path

```text
mainnet / testnet-10 kaspad
        │ wRPC: virtual chain, accepted tx ids, blocks, mempool
        ▼
node::ChainSource boundary
        │ converts rusty-kaspa types into kascov model types
        ▼
sync.rs
        │ resolve accepted bodies → classify → rollback/apply atomically
        ▼
store.rs / SQLite
        │ canonical events, cells, reveals, payload stamps, token derivations
        ├──────────────► CLI reads: list / show / trace / watch / export
        │
        ▼
kascov serve
        │ JSON, SSE, analytics, search, debugger, builder, share surfaces
        ▼
Caddy
        │ same-origin proxy + static files + privacy-filtered access logs
        ▼
web/index.html + app.js + core/*
        │ route-specific fetch/cache/reconcile/render
        ▼
explorer, API docs, builder guide, playground, tokens, galaxy
```

The important trust boundary is between `node/` and `model.rs`. Upstream consensus/RPC types stop there. Everything after that boundary operates on stable project-owned types.

## Workspace ownership

| component | owns | must not own |
|---|---|---|
| `kascov-core/node` | RPC, upstream conversion, consensus covenant-id helper | storage or UI policy |
| `kascov-core/sync.rs` | acceptance order, classification, reorg/recovery orchestration | HTTP representation |
| `kascov-core/store.rs` | transactions, persistence, query shapes, derived ledgers | node calls |
| `kascov-decode` | script facts, verified reveals, deterministic recognition | chain truth or persistence |
| `kascov-sim` | script-engine execution and replay | broadcasting |
| `kascov-labkit` | building/signing covenant transactions | CLI presentation |
| `kascov-lab` | operator-facing testnet workflows | production serving |
| `kascov` CLI/serve | command UX, HTTP API, background tasks | consensus reimplementation |
| `web/core` | browser state, data, loading, routing, refresh policies | DOM markup |
| `web/app.js` | route rendering and interaction orchestration | durable truth |
| `scripts/` | deployment, reporting, digest, recovery helpers | product data model |

## Truth hierarchy

When two sources disagree, kascov follows this order:

1. consensus-accepted chain data;
2. locally persisted accepted history;
3. deterministic derivations from that history;
4. verified external bytes, such as token art matching an on-chain hash;
5. explicitly labeled claims, such as a deployer-provided token name;
6. presentation estimates and heuristics.

Examples:

- a KIP-20 id is recomputed with the consensus helper, not inferred from shape;
- a P2SH reveal is shown only after its program hashes to the commitment;
- KCC20 is “verified” only when every state transition and live cell is provable;
- a ZK system label based on push sizes is described as a heuristic;
- missing pre-capture timestamps remain missing or estimated rather than invented.

## State machines

### Covenant lifecycle

```text
unknown
  ├─ valid new id ─► genesis ─► transition* ─► burn
  └─ first seen mid-life ─► transition* ─► burn
                           lineage_complete = false
```

A covenant is alive when at least one bound UTXO is unspent. Multiple live cells are legal. “Burn” means the accepted transaction consumed the covenant's state without producing a successor for that id.

### Sync lifecycle

```text
read tip → read stored cursor → virtual-chain diff
  ├─ removed blocks → rollback
  ├─ added blocks   → prefetch → resolve → classify → apply
  └─ no events      → periodic/final checkpoint
```

The cursor and accepted data advance in one transaction. A crash therefore resumes before or after a block, never halfway through its covenant effects.

### Browser lifecycle

```text
route → loading policy
  ├─ landing/explore → tiny live paint + large snapshot
  └─ dedicated view → small endpoint/static markup

SSE/poll event → keyed gate → shared transfer → cache update
  └─ rerender only if the current view is safe to rebuild
```

Navigation effects—scroll reset, entrance animation, focus announcement—belong to route changes, not data refreshes.

## Failure containment

| failure | containment |
|---|---|
| node temporarily omits an accepted body at the tip | fail the sync pass; reconnect and retry rather than skip history |
| virtual-chain cursor exists but cannot be walked | progress watchdog demands `re_anchor` |
| old history is pruned | mark lineage incomplete; gap recovery can merge only what an archival source still has |
| process dies during apply | SQLite transaction preserves cursor/data consistency |
| optional HTTP endpoint is absent | web feature-detects `404`, caches the miss, and hides the panel |
| SSE bursts during a large fetch | keyed request/refresh sharing prevents duplicate transfers |
| background data arrives during form use | self-rendered views are not rebuilt |
| Telegram credentials are absent | digest prints to CI log and succeeds with a warning |
| scanner noise hits production | traffic report classifies it as bot traffic |

## Public surfaces

- CLI: local indexing, inspection, export, backup, recovery.
- HTTP API: open JSON/SSE reads plus narrowly gated write tools.
- Explorer: live network, pending lane, coin/transaction/address/token pages.
- Builder tools: decode, compile, simulate, preflight, debug, publish, deploy.
- Discovery: galaxy, analytics, lanes, inscriptions, changelog, Atom feed.
- Sharing: crawler pages, OG images, badges, sitemap.
- Client libraries: `clients/js/` and `clients/py/`.

## Where to go next

- Correctness and reorgs: [[Sync Engine]]
- Persistence and derivations: [[Storage Schema]]
- Script interpretation: [[Decoding]]
- Browser behavior: [[Web Explorer]]
- Production procedures: [[Operations]]
- Test layers: [[Testing Strategy]]
- Protocol facts: [[Toccata Protocol Notes]]
