//! The OpenAPI document for the worker's public surface, served by the
//! worker itself so the contract ships inside the build that implements it
//! (`info.version` is the same KASCOV_GIT_HASH /healthz answers). The site's
//! own audit found the docs drifting behind the router; the tests here parse
//! the router out of main.rs and refuse a document that says less — or more —
//! than the routes do.

use serde_json::{json, Value};

/// One serialization per process: the document is a pure function of the
/// build, so the first request pays the ~ms of json! assembly and every
/// later one hands out the same bytes.
static DOC_JSON: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// GET handler for the document. Cached like /data/{network}/index.json:
/// the body only changes with a deploy, and the build-hash version lets a
/// consumer detect that without conditional requests.
pub async fn openapi_handler() -> axum::response::Response {
    use axum::http::header;
    use axum::response::IntoResponse;
    let body: &'static str = DOC_JSON.get_or_init(|| document().to_string());
    (
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
        ],
        body,
    )
        .into_response()
}

/* ------------------------------------------------------------- builders */

fn param(name: &str, location: &str, required: bool, schema: Value, description: &str) -> Value {
    json!({
        "name": name,
        "in": location,
        "required": required,
        "schema": schema,
        "description": description,
    })
}

/// Every /data/{network}/… route resolves the segment the same way: a
/// network this worker follows, or a plain 404 — unknown networks are
/// refused, never guessed.
fn network_param() -> Value {
    param(
        "network",
        "path",
        true,
        json!({ "type": "string", "enum": ["mainnet", "testnet-10"] }),
        "A network this worker follows. Anything else answers 404 \"unknown network\".",
    )
}

fn covenant_id_param(description: &str) -> Value {
    param(
        "id",
        "path",
        true,
        json!({ "type": "string", "pattern": "^[0-9a-f]{64}$" }),
        description,
    )
}

fn txid_param() -> Value {
    param(
        "txid",
        "path",
        true,
        json!({ "type": "string", "pattern": "^[0-9a-f]{64}$" }),
        "Transaction id, 64 hex characters.",
    )
}

/// Response map from (status, description) pairs — description-only entries
/// for the surfaces whose exact body shape is easiest read from the endpoint
/// itself, with full schemas reserved for the machine-consumed feeds below.
fn responses(items: &[(&str, &str)]) -> Value {
    let mut map = serde_json::Map::new();
    for (code, description) in items {
        map.insert(code.to_string(), json!({ "description": description }));
    }
    Value::Object(map)
}

fn op(summary: &str, description: &str, parameters: Value, responses: Value) -> Value {
    json!({
        "summary": summary,
        "description": description,
        "parameters": parameters,
        "responses": responses,
    })
}

fn post_op(
    summary: &str,
    description: &str,
    parameters: Value,
    body_schema: Value,
    responses: Value,
) -> Value {
    json!({
        "summary": summary,
        "description": description,
        "parameters": parameters,
        "requestBody": {
            "required": true,
            "content": { "application/json": { "schema": body_schema } },
        },
        "responses": responses,
    })
}

/// The exact two integers a trade committed — total sompi over tokens.
/// Candles and the book publish the pair itself, never its quotient, which
/// would collapse distinct price levels.
fn price_pair_schema() -> Value {
    json!({
        "type": "object",
        "description": "An exact price as the pair the chain bytes state: total sompi asked over tokens moved. Divide only for display; comparisons belong on the cross-multiplied pair.",
        "required": ["quote_sompi", "base_amount"],
        "properties": {
            "quote_sompi": { "type": "integer" },
            "base_amount": { "type": "integer" },
        },
    })
}

/// The SPA shell routes share one operation: per-route head metadata, same
/// body contract.
fn shell_op(summary: &str) -> Value {
    op(
        summary,
        "The SPA shell with this route's head metadata. Behind Firebase these paths are answered \
         by hosting; a front door that proxies the worker directly gets indexable pages from here.",
        json!([]),
        responses(&[("200", "text/html — the app shell.")]),
    )
}

/* ------------------------------------------------------------- document */

/// The whole document. Paths are listed in router order (main.rs) so a
/// side-by-side diff of the two stays mechanical.
pub fn document() -> Value {
    let mut paths = serde_json::Map::new();
    let mut add = |path: &str, item: Value| {
        paths.insert(path.to_string(), item);
    };

    // --- health -----------------------------------------------------------
    let healthz = op(
        "Follower liveness and index progress per network",
        "503 as soon as ANY followed network hasn't completed a sync pass in 10 minutes, or keeps \
         completing passes without advancing while the index lags far behind the tip — the uptime \
         check's restart signal. /health is the alias that survives Google Front End swallowing \
         /healthz on *.run.app. `no-store`.",
        json!([]),
        json!({
            "200": {
                "description": "All followed networks are syncing and advancing.",
                "content": { "application/json": { "schema": {
                    "type": "object",
                    "required": ["status", "build", "networks"],
                    "properties": {
                        "status": { "type": "string", "enum": ["ok", "stalled"] },
                        "build": {
                            "type": "string",
                            "description": "The short git hash this binary was built from. The deploy script asserts it equals HEAD after every rollout, so a stale value here is a failed deploy, not a cosmetic string.",
                        },
                        "networks": {
                            "type": "object",
                            "description": "Per followed network: status, processed_daa/tip_daa/lag_daa (nulls until the follower has created the database), last_sync_ok_ms, last_progress_ms, tx_index_backfill_done, and the mempool poller's own health (additive product feed — a disabled poller never turns into a worker-wide 503).",
                        },
                    },
                } } },
            },
            "503": { "description": "At least one followed network is stalled; same body shape with status \"stalled\"." },
        }),
    );
    // /health duplicates /healthz's operation (one handler serves both).
    // Cloned rather than cross-referenced: paths stay self-contained for
    // consumers that read one entry at a time.
    let mut health_alias = healthz.clone();
    health_alias["summary"] = json!("Alias of /healthz for fronts that swallow that path");
    add("/healthz", json!({ "get": healthz }));
    add("/health", json!({ "get": health_alias }));

    // --- shell ------------------------------------------------------------
    add("/", json!({ "get": shell_op("The explorer shell") }));
    add("/guide", json!({ "get": shell_op("The guide shell") }));
    add("/dev", json!({ "get": shell_op("The developer docs shell") }));
    add("/tokens", json!({ "get": shell_op("The token directory shell") }));
    add("/pools", json!({ "get": shell_op("The pools shell") }));

    // --- tools (POST) -----------------------------------------------------
    add("/data/{network}/simulate", json!({ "post": post_op(
        "Run a hypothetical covenant spend through the real script engine",
        "Off-chain execution via kascov-sim — the actual Kaspa engine, not a model. \
         Network-agnostic (pure computation); the {network} segment just keeps it under the /data \
         rewrite. Programs above 20000 hex characters are refused.",
        json!([network_param()]),
        json!({
            "type": "object",
            "required": ["program_hex", "entrypoint"],
            "properties": {
                "program_hex": { "type": "string", "description": "The coin's compiled program, hex." },
                "entrypoint": { "type": "string", "description": "Which entrypoint to satisfy: spend | reclaim | cold | inherit." },
                "recipient": { "type": "string", "description": "Where the funds go: buyer | seller | other | self." },
                "value": { "type": "integer", "description": "The state coin's value, in sompi." },
            },
            "description": "kascov-sim's SimRequest; unlisted optional fields keep their documented defaults there.",
        }),
        responses(&[
            ("200", "The engine's verdict, pass or fail with the reason. `no-store`."),
            ("400", "Program too large."),
        ]),
    ) }));

    add("/data/{network}/preflight", json!({ "post": post_op(
        "\"Will this transaction pass?\" before broadcast",
        "SDK/RPC transaction JSON in; trap findings, consensus masses, and (when the inputs carry \
         enough context) a real engine execution out. Pure computation, but engine runs burn CPU — \
         covered by the shared tool limiter like the other compiler-adjacent endpoints. Bodies over \
         256 KiB are refused by the extractor: a whole transaction with witnesses fits comfortably \
         below that.",
        json!([network_param()]),
        json!({ "type": "object", "description": "A transaction in Kaspa SDK/RPC JSON shape (see crate::preflight)." }),
        responses(&[
            ("200", "{ok, findings, masses, …} — or {ok:false, error} for a body the checker can't read."),
            ("413", "Body over the 256 KiB cap."),
            ("429", "The shared tool limiter is saturated."),
        ]),
    ) }));

    add("/data/{network}/zk-verify", json!({ "post": post_op(
        "Run a self-contained ZK verification script through the real engine",
        "Kaspa's ark_groth16 / RISC-Zero verifier path. Programs above 8000 hex characters are \
         refused.",
        json!([network_param()]),
        json!({
            "type": "object",
            "required": ["program_hex"],
            "properties": { "program_hex": { "type": "string" } },
        }),
        responses(&[
            ("200", "{ok, …} — verification failures are 200 with ok:false and the reason: the request was well formed and the answer is simply no."),
            ("429", "The shared tool limiter is saturated."),
        ]),
    ) }));

    add("/data/{network}/compile", json!({ "post": post_op(
        "Compile SilverScript source + constructor args to script hex",
        "Shells out to the silverc binary. Powers verify-and-publish and the no-code builder.",
        json!([network_param()]),
        compile_body_schema(),
        responses(&[
            ("200", "{ok, script_hex, …} — compile errors are 200 with ok:false and the compiler's message."),
            ("429", "The shared tool limiter is saturated."),
        ]),
    ) }));

    add("/data/{network}/deploy", json!({ "post": post_op(
        "Deploy a compiled program as a covenant coin (custodial faucet, testnet-10 only)",
        "Gated OFF by default: the route answers 404 unless the worker is armed with a deploy key, \
         and only ever on testnet-10. Value bounds 1..10 TKAS (in sompi) keep a runaway request \
         from draining the faucet into one coin; per-IP and global rate limits before the node is \
         touched.",
        json!([network_param()]),
        json!({
            "type": "object",
            "required": ["program_hex"],
            "properties": {
                "program_hex": { "type": "string" },
                "value": { "type": "integer", "description": "Sompi, 100000000..=1000000000." },
            },
        }),
        responses(&[
            ("200", "{ok, covenant_id, network} on success; {ok:false, error} for refused bodies."),
            ("404", "Unknown network, or the worker is not armed to deploy."),
            ("429", "Deploy rate limit reached."),
        ]),
    ) }));

    add("/data/{network}/publish", json!({ "post": post_op(
        "Publish source for a program hash (verify-and-publish)",
        "Compiles the submitted source, and only if it compiles records it as a community-verified \
         source keyed by the program's blake2b hash. A coin whose revealed program hashes the same \
         then shows this source. Sources above 40000 characters are refused.",
        json!([network_param()]),
        compile_body_schema(),
        responses(&[
            ("200", "{ok, program_hash, …} — or {ok:false, error}."),
            ("429", "The shared tool limiter is saturated."),
        ]),
    ) }));

    add("/data/{network}/verified/{hash}", json!({ "get": op(
        "The published source for a program hash",
        "What /publish recorded, keyed by the program's blake2b hash. `.json` suffix accepted.",
        json!([network_param(), param("hash", "path", true, json!({ "type": "string" }),
            "Program blake2b hash, hex.")]),
        responses(&[
            ("200", "{ok:true, source, args, template, verified_at} — or {ok:false} for a hash nothing was published under."),
            ("404", "Unknown network."),
        ]),
    ) }));

    add("/data/{network}/subscribe", json!({ "post": post_op(
        "Register a webhook for covenant events",
        "Delivery does an SSRF pre-flight and retries up to 3 times per event. The returned secret \
         signs every delivery and is required to unsubscribe — keep it.",
        json!([network_param()]),
        json!({
            "type": "object",
            "required": ["url"],
            "properties": {
                "url": { "type": "string", "description": "http(s) delivery URL." },
                "covenant_id": { "type": "string", "description": "Optional 64-hex filter; omitted means all covenants." },
                "kind": { "type": "string", "enum": ["genesis", "transition", "burn"], "description": "Optional kind filter; omitted means all kinds." },
            },
        }),
        responses(&[
            ("200", "{ok:true, id, secret} — or {ok:false, error} for a refused url or filter."),
            ("404", "Unknown network."),
        ]),
    ) }));

    add("/data/{network}/unsubscribe", json!({ "post": post_op(
        "Remove a webhook subscription",
        "By the {id, secret} /subscribe returned. Legacy rows created before secrets existed still \
         delete by id alone.",
        json!([network_param()]),
        json!({
            "type": "object",
            "required": ["id"],
            "properties": {
                "id": { "type": "integer" },
                "secret": { "type": "string" },
            },
        }),
        responses(&[
            ("200", "{ok:true} — or {ok:false, error}."),
            ("404", "Unknown network."),
        ]),
    ) }));

    // --- holder lane ------------------------------------------------------
    add("/data/{network}/lane", json!({ "get": op(
        "The published holder-lane policy",
        "Capacities per tier, token expiry, and how to mint a pass. Holder capacity is additive; \
         the anonymous tier is a floor that can only rise; lane tokens are stateless and nothing \
         is stored.",
        json!([network_param()]),
        responses(&[
            ("200", "The policy object, including token_expiry_days and the mint instructions."),
            ("404", "Unknown network."),
        ]),
    ) }));

    add("/data/{network}/lane/mint", json!({ "post": post_op(
        "Mint a 30-day stateless holder-lane pass",
        "Sign a nonce, prove the key holds KASCOV, get a pass. The proof is judged by the same \
         signature oracle as /prove-holding, and the balance is read from the chain index, never \
         claimed. kascov keeps no list of holders: the pass is the whole record, and losing it \
         just means signing again.",
        json!([network_param()]),
        json!({
            "type": "object",
            "required": ["address", "nonce", "signature"],
            "properties": {
                "address": { "type": "string" },
                "nonce": { "type": "string" },
                "signature": { "type": "string", "description": "64-byte schnorr signature, hex." },
            },
        }),
        responses(&[
            ("200", "{ok:true, token, …} — or {ok:false, error}; a failed proof is 200, the answer is simply no."),
            ("429", "The shared tool limiter is saturated."),
        ]),
    ) }));

    add("/data/{network}/lane/{ns}", json!({ "get": op(
        "One KIP-21 lane namespace's dashboard",
        "Static /lane and /lane/mint win route priority over this capture — a KIP-21 namespace is \
         8 hex characters, so neither was ever a lane.",
        json!([network_param(), param("ns", "path", true, json!({ "type": "string" }),
            "KIP-21 namespace, 8 hex characters.")]),
        responses(&[
            ("200", "The namespace's covenants and activity."),
            ("404", "Unknown network or namespace."),
        ]),
    ) }));

    add("/data/{network}/debug/{txid}", json!({ "get": op(
        "Replay a REAL on-chain covenant spend",
        "The transaction's actual program and witness re-run through the engine, step by step — \
         what happened, proven by running it again.",
        json!([network_param(), txid_param()]),
        responses(&[
            ("200", "The replay trace."),
            ("404", "Unknown network or transaction."),
        ]),
    ) }));

    // --- feeds ------------------------------------------------------------
    add("/data/price.json", json!({ "get": op(
        "KAS/USD spot for the UI",
        "Network-independent, cached with a single-flight upstream fetch. The one number on the \
         site not proven from chain, which is why the body names its source.",
        json!([]),
        responses(&[("200", "{kas_usd, updated_at_ms, source}.")]),
    ) }));

    add("/data/{file}", json!({ "get": op(
        "The explorer grid snapshot or the live feed",
        "{network}.json is the explorer grid (summaries only); {network}-live.json the small \
         fast-changing feed. Full timelines live at /data/{network}/c/{id}.json, one covenant at \
         a time.",
        json!([param("file", "path", true, json!({ "type": "string" }),
            "\"{network}.json\" or \"{network}-live.json\".")]),
        responses(&[
            ("200", "The requested snapshot."),
            ("404", "Not a followed network's snapshot name."),
        ]),
    ) }));

    add("/data/{network}/c/{id}", json!({ "get": op(
        "One covenant's full detail",
        "Summary, decoded state timeline, and everything else the site's coin page shows. `.json` \
         suffix accepted.",
        json!([network_param(), covenant_id_param("Covenant id, 64 hex characters.")]),
        responses(&[
            ("200", "The covenant detail object."),
            ("400", "Malformed id — garbage never populates the cache."),
            ("404", "Unknown network or covenant."),
        ]),
    ) }));

    add("/data/{network}/template/{hash}", json!({ "get": op(
        "The covenants whose reveals proved a KCC-1 TemplateHash",
        "KCC-1 draft §8.3 template lookup: which deployed programs hash to this template.",
        json!([network_param(), param("hash", "path", true, json!({ "type": "string" }),
            "TemplateHash, hex.")]),
        responses(&[
            ("200", "The template's covenants."),
            ("400", "Non-hex input — garbage never populates the cache."),
            ("404", "Unknown network or hash."),
        ]),
    ) }));

    add("/data/{network}/tx/{txid}", json!({ "get": op(
        "Everything kascov saw one transaction do",
        "The covenant events it fired, the state cells it created and spent, and the classified \
         token deltas riding those events. `covenant_id`/`covenant_ids` stay for existing \
         consumers; everything else is additive.",
        json!([network_param(), txid_param()]),
        responses(&[
            ("200", "The transaction's covenant view."),
            ("404", "Unknown network or transaction."),
        ]),
    ) }));

    add("/data/{network}/families.json", json!({ "get": op(
        "Covenants that moved together, clustered into apps",
        "Union-find over transactions that touched more than one covenant — multi-contract flows \
         surface as one family.",
        json!([network_param()]),
        responses(&[("200", "The family clusters."), ("404", "Unknown network.")]),
    ) }));

    add("/data/{network}/reorgs.json", json!({ "get": op(
        "The applied virtual-chain reorg feed",
        "Reorgs the follower actually applied to the index, newest first.",
        json!([network_param()]),
        responses(&[("200", "The reorg feed."), ("404", "Unknown network.")]),
    ) }));

    add("/data/{network}/galaxy.json", json!({ "get": op(
        "The whole-network App Graph",
        "Precomputed positions, weighted edges, and status. Opt-in payload variants: unknown \
         params and unknown values degrade to the legacy shape, so old and new clients both work.",
        json!([network_param(),
            param("fmt", "query", false, json!({ "type": "string", "enum": ["2"] }),
                "\"2\" selects the columnar payload."),
            param("tier", "query", false, json!({ "type": "string", "enum": ["core", "visual"] }),
                "Subset tiers; \"visual\" only exists columnar."),
        ]),
        responses(&[("200", "The graph."), ("404", "Unknown network.")]),
    ) }));

    add("/data/{network}/lanes.json", json!({ "get": op(
        "Activity lanes",
        "Recognized templates and inscription kinds ranked by events, with tip_daa for freshness.",
        json!([network_param()]),
        responses(&[("200", "The lanes."), ("404", "Unknown network.")]),
    ) }));

    add("/data/{network}/inscriptions.json", json!({ "get": op(
        "Inscription breakdown",
        "Per recognized inscription label: event and covenant counts.",
        json!([network_param()]),
        responses(&[("200", "{inscriptions: [{label, events, covenants}]}."), ("404", "Unknown network.")]),
    ) }));

    add("/data/{network}/lifespans.json", json!({ "get": op(
        "Coin lifespan histogram",
        "Bucketed lifespans plus the median, in DAA and its ~100ms/DAA wall-clock estimate.",
        json!([network_param()]),
        responses(&[("200", "{buckets, median_daa, median_ms, total}."), ("404", "Unknown network.")]),
    ) }));

    add("/data/{network}/digest.json", json!({ "get": op(
        "The last 24 hours as one small object",
        "Counts, value born, and the headline coins. A daily summary moves slowly; the CDN absorbs \
         the herd.",
        json!([network_param()]),
        responses(&[("200", "The digest."), ("404", "Unknown network.")]),
    ) }));

    add("/data/{network}/templates.json", json!({ "get": op(
        "Contract-type analytics",
        "What runs on this network, by recognized script template.",
        json!([network_param()]),
        responses(&[("200", "The template counts."), ("404", "Unknown network.")]),
    ) }));

    add("/data/{network}/tokens.json", json!({ "get": op(
        "The derived KCC20 token directory",
        "Every token with its validation verdict, proven supply/holders where provable, plus the \
         minter/vault covenants with the token ids they pin. Reads only the precomputed token \
         tables.",
        json!([network_param()]),
        responses(&[("200", "The directory."), ("404", "Unknown network.")]),
    ) }));

    add("/data/{network}/verification.json", json!({ "get": op(
        "The verification log and the to-audit queue",
        "A record of what ran, not an authority on what may be published: every figure on the site \
         is re-proved from chain each time it is served. The queue of unmatched programs is ranked \
         by how much activity rides on each — more at stake if it stays unaudited, never more \
         trustworthy. Includes the newest audit-bench report when one exists.",
        json!([network_param()]),
        responses(&[("200", "{runs, unknown_builds, unknown_programs_total, unknown_covenants_total, audit_bench, …}."), ("404", "Unknown network.")]),
    ) }));

    // --- tokens -----------------------------------------------------------
    add("/data/{network}/token/{id}/trades.json", json!({ "get": op(
        "Every verified trade for one token, newest first",
        "Separate from the token page on purpose: the full list can run to thousands of rows, and \
         nobody should pay for that on a page load they did not ask for.",
        json!([network_param(), covenant_id_param("Token (covenant) id, 64 hex characters.")]),
        responses(&[
            ("200", "The trades."),
            ("400", "Malformed id."),
            ("404", "Unknown network or token."),
        ]),
    ) }));

    add("/data/{network}/token/{id}/candles", json!({ "get": op(
        "OHLC+volume buckets for one token's market",
        "Aggregates exactly the trades market_summary's filter admits — same-tx-clean, on the \
         token's one market covenant, bracket-passing under the audited fee model. A market that \
         fails a pricing gate serves an empty list plus the reason; kascov prices nothing it \
         cannot prove.",
        json!([network_param(), covenant_id_param("Token (covenant) id, 64 hex characters."),
            param("bucket", "query", false, json!({ "type": "string", "enum": ["1h", "4h", "1d"], "default": "1h" }),
                "Bucket width. An allowlist, never parsed arithmetic — an arbitrary width would let a caller mint unbounded cache keys."),
        ]),
        json!({
            "200": {
                "description": "The candle series, oldest first — or empty with `reason` when a pricing gate refuses.",
                "content": { "application/json": { "schema": {
                    "type": "object",
                    "required": ["network", "token_id", "bucket", "bucket_ms", "generated_at_ms", "provenance", "candles"],
                    "properties": {
                        "network": { "type": "string" },
                        "token_id": { "type": "string" },
                        "bucket": { "type": "string", "enum": ["1h", "4h", "1d"] },
                        "bucket_ms": { "type": "integer" },
                        "generated_at_ms": { "type": "integer" },
                        "provenance": {
                            "type": "string",
                            "description": "Restates the admission rule every candle passed. Served with the data because a consumer forwarding a candle should be able to forward its terms.",
                        },
                        "reason": {
                            "type": "string",
                            "description": "Present only when the series is refused (unverified derivation, LP share token, no single market covenant, no audited fee model, invariant violation, or too few exercised trades). candles is then empty.",
                        },
                        "candles": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "required": ["t", "open", "high", "low", "close", "trades", "first_txid", "last_txid"],
                                "properties": {
                                    "t": { "type": "integer", "description": "Bucket start, unix ms." },
                                    "open": price_pair_schema(),
                                    "high": price_pair_schema(),
                                    "low": price_pair_schema(),
                                    "close": price_pair_schema(),
                                    "volume_sompi": {
                                        "type": ["integer", "null"],
                                        "description": "Sompi traded in the bucket; null if the i128 sum ever exceeded i64 — a null beats a misstated number.",
                                    },
                                    "trades": { "type": "integer" },
                                    "first_txid": {
                                        "type": "string",
                                        "description": "The bucket's opening trade's transaction — replay it at /data/{network}/debug/{txid}.",
                                    },
                                    "last_txid": {
                                        "type": "string",
                                        "description": "The bucket's closing trade's transaction — every candle ties to replayable transactions.",
                                    },
                                },
                            },
                        },
                    },
                } } },
            },
            "400": { "description": "Malformed token id, or unknown bucket (\"use 1h | 4h | 1d\")." },
            "404": { "description": "Unknown network, or not a token the derivation knows." },
        }),
    ) }));

    add("/data/{network}/token/{id}/book", json!({ "get": op(
        "The open resting orders naming this token",
        "DECODED facts with their provenance stated: each row restates what a committed order \
         program's own bytes offer, and nothing here has passed (or could pass) the market \
         verification gate. An empty book is empty arrays, never a 404 — \"no orders\" is a fact \
         worth serving.",
        json!([network_param(), covenant_id_param("Token (covenant) id, 64 hex characters.")]),
        json!({
            "200": {
                "description": "Both sides, best price first: bids highest, asks lowest, ordered on the exact pair via cross-multiplication.",
                "content": { "application/json": { "schema": {
                    "type": "object",
                    "required": ["network", "token_id", "generated_at_ms", "provenance", "bids", "asks"],
                    "properties": {
                        "network": { "type": "string" },
                        "token_id": { "type": "string" },
                        "generated_at_ms": { "type": "integer" },
                        "provenance": {
                            "type": "string",
                            "description": "\"decoded, not verified\": the rows restate the one offer each order program's bytes state (price pair, size, expiry); kascov has not verified any spend against these prices, and nothing here is a quote.",
                        },
                        "bids": { "$ref": "#/components/schemas/RestingOrders" },
                        "asks": { "$ref": "#/components/schemas/RestingOrders" },
                    },
                } } },
            },
            "400": { "description": "Malformed token id." },
            "404": { "description": "Unknown network." },
        }),
    ) }));

    add("/data/{network}/token/{id}", json!({ "get": op(
        "One derived token",
        "Directory row, top holders, the classified event-delta history, and the validation \
         summary. The history reads oldest first by default (exclusive after_seq cursor, \
         next_after_seq when more remain); order=desc or a before_seq cursor reads newest first \
         and returns next_before_seq. A page always cuts on a whole-event boundary.",
        json!([network_param(), covenant_id_param("Token (covenant) id, 64 hex characters."),
            param("limit", "query", false, json!({ "type": "integer", "maximum": 500 }),
                "Holder rows to return."),
            param("events_limit", "query", false, json!({ "type": "integer", "maximum": 1000, "default": 200 }),
                "Event-delta page size."),
            param("after_seq", "query", false, json!({ "type": "integer" }),
                "Exclusive cursor for oldest-first pages."),
            param("before_seq", "query", false, json!({ "type": "integer" }),
                "Exclusive cursor for newest-first pages; implies descending order."),
            param("order", "query", false, json!({ "type": "string", "enum": ["asc", "desc"] }),
                "History direction; default oldest first."),
        ]),
        responses(&[
            ("200", "The token object."),
            ("400", "Malformed id."),
            ("404", "Unknown network, or an id the derivation doesn't know as a token."),
        ]),
    ) }));

    // --- feeds (continued) ------------------------------------------------
    add("/data/{network}/consistency.json", json!({ "get": op(
        "The latest cross-indexer consistency report",
        "kascov's index tested against other indexers' published answers — discrepancies are \
         findings, not accusations.",
        json!([network_param()]),
        responses(&[("200", "The report."), ("404", "Unknown network.")]),
    ) }));

    add("/data/{network}/events", json!({ "get": op(
        "The chain-wide event feed",
        "Canonical event objects in their canonical deterministic order (accepting_daa, tx_index \
         NULLS LAST, txid), oldest first. When more rows remain the response carries \
         next_after_daa/next_after_seq — feed them back verbatim to keep walking; after_seq counts \
         events already consumed inside the after_daa group, so the offset is stable.",
        json!([network_param(),
            param("after_daa", "query", false, json!({ "type": "integer" }),
                "Resume after this accepting DAA score."),
            param("after_seq", "query", false, json!({ "type": "integer" }),
                "Events already consumed inside the after_daa group."),
            param("limit", "query", false, json!({ "type": "integer", "maximum": 1000, "default": 200 }),
                "Page size."),
        ]),
        responses(&[
            ("200", "The page, with the cursor when more remain."),
            ("400", "Malformed cursor or limit."),
            ("404", "Unknown network."),
        ]),
    ) }));

    add("/data/{network}/coins", json!({ "get": op(
        "Batch compact summaries",
        "Unknown ids are simply omitted; malformed input is a 400. Deliberately uncached: ids is \
         an unbounded keyspace, and each id is one indexed lookup.",
        json!([network_param(),
            param("ids", "query", true, json!({ "type": "string" }),
                "Comma-separated covenant ids, 64 hex each."),
            param("fields", "query", false, json!({ "type": "string", "enum": ["summary"] }),
                "Projection; only \"summary\" exists."),
        ]),
        responses(&[
            ("200", "The found summaries."),
            ("400", "Malformed ids."),
            ("404", "Unknown network."),
        ]),
    ) }));

    add("/data/{network}/activity.json", json!({ "get": op(
        "Kind counts per DAA bucket",
        "The interactive activity chart's data. range is an allowlist; unknown values are a 400.",
        json!([network_param(),
            param("range", "query", false, json!({ "type": "string", "enum": ["1h", "6h", "24h", "48h", "all"], "default": "24h" }),
                "Window; \"all\" derives its width from the index's own bounds."),
        ]),
        responses(&[
            ("200", "The buckets."),
            ("400", "Unknown range — use 1h | 6h | 24h | 48h | all."),
            ("404", "Unknown network."),
        ]),
    ) }));

    add("/data/{network}/addr/{address}", json!({ "get": op(
        "What one key owns, holds, and traded",
        "Three separate indexes, all proven from chain: covenant cells the key is p2pk owner of, \
         token balances (a holder can own millions of a token without owning a cell), and recent \
         trades (a key that bought and sold out shows in neither of the first two). `.json` suffix \
         accepted; address form and pubkey hex share one cache entry.",
        json!([network_param(), param("address", "path", true, json!({ "type": "string" }),
            "A kaspa address or 32/33-byte pubkey hex.")]),
        responses(&[
            ("200", "Covenants, holdings, and trades for the key."),
            ("400", "Not a kaspa address or 32/33-byte pubkey hex."),
            ("404", "Unknown network."),
        ]),
    ) }));

    add("/data/{network}/prove-holding", json!({ "post": post_op(
        "Did this key sign this exact message, and what does it hold?",
        "Stateless by design: the CHALLENGE lives with the caller, who picks the nonce, binds it \
         to the account it is about to grant to, and remembers what it issued. A replayed body is \
         worthless — it returns the same public fact it always did, and the thing that decides is \
         the caller's own record of which nonce it gave to whom. A failed proof is a 200 with \
         verified:false: the request was well formed and the answer is simply no.",
        json!([network_param()]),
        json!({
            "type": "object",
            "required": ["address", "nonce", "signature"],
            "properties": {
                "address": { "type": "string" },
                "nonce": { "type": "string" },
                "signature": { "type": "string", "description": "64-byte schnorr signature, hex." },
            },
        }),
        responses(&[
            ("200", "{verified, holdings…} — verified:false for a proof that doesn't hold."),
            ("429", "The shared tool limiter is saturated."),
        ]),
    ) }));

    add("/data/{network}/search", json!({ "get": op(
        "Find covenants by id prefix, name, claim, listing, or template",
        "Each row says WHY it matched, because the lanes carry different provenance: a deployer's \
         own on-chain claim outranks a third party's list for the same covenant, and only tokens \
         kascov indexed itself ever appear.",
        json!([network_param(),
            param("q", "query", true, json!({ "type": "string", "minLength": 1, "maxLength": 64 }),
                "The query, matched case-insensitively."),
            param("limit", "query", false, json!({ "type": "integer", "minimum": 1, "maximum": 20, "default": 10 }),
                "Rows to return."),
        ]),
        json!({
            "200": {
                "description": "The matches, higher-trust lanes first.",
                "content": { "application/json": { "schema": {
                    "type": "object",
                    "required": ["network", "query", "results"],
                    "properties": {
                        "network": { "type": "string" },
                        "query": { "type": "string" },
                        "results": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "required": ["id", "name", "template", "status", "matched"],
                                "properties": {
                                    "id": { "type": "string" },
                                    "name": { "type": "string", "description": "The canonical slug kascov derives from the id — never a claim." },
                                    "template": { "type": "string" },
                                    "status": { "type": "string", "enum": ["active", "burned"] },
                                    "matched": {
                                        "type": "string",
                                        "enum": ["id", "name", "claimed", "listed", "template"],
                                        "description": "The lane that matched, i.e. the row's provenance: \"id\" the covenant id's hex prefix; \"name\" the canonical slug or one of its tokens; \"claimed\" the deployer's own on-chain claimed name/ticker (the deployer's assertion, not a verified name); \"listed\" a registry-listed display name/ticker — a third party's signed list, cross-checked against the index but never consumed as truth, ranked below claimed hits by construction; \"template\" a recognized template substring.",
                                    },
                                    "claimed": { "type": "string", "description": "For matched:\"claimed\": the claimed string that matched — without it the row has no visible reason to be in the results." },
                                    "listed": { "type": "string", "description": "For matched:\"listed\": the listed name that matched, carried for the same reason claimed is." },
                                },
                            },
                        },
                    },
                } } },
            },
            "400": { "description": "q must be 1..=64 characters." },
            "404": { "description": "Unknown network." },
            "503": { "description": "Search unavailable." },
        }),
    ) }));

    add("/data/{network}/stream", json!({ "get": op(
        "Covenant events over SSE, the moment the follower indexes them",
        "Hints only — no replay, no backlog, lagged subscribers skip ahead; consumers confirm \
         state through the polled feeds. Same-origin on kascov.io, unbuffered.",
        json!([network_param(),
            param("covenant", "query", false, json!({ "type": "string", "pattern": "^[0-9a-f]{64}$" }),
                "Filter to one covenant id. A typo'd filter fails loudly (400), never silently streams the whole firehose."),
        ]),
        responses(&[
            ("200", "text/event-stream."),
            ("400", "Malformed covenant filter."),
            ("404", "Unknown network."),
        ]),
    ) }));

    add("/data/{network}/pending", json!({ "get": op(
        "Live pending covenant transactions and poller health",
        "In-memory mempool snapshot — `no-store`, it changes every poll. Legacy pending rows and \
         their scalar event fields remain; health/revision and per-tx events are additive.",
        json!([network_param()]),
        responses(&[("200", "The snapshot."), ("404", "Unknown network.")]),
    ) }));

    add("/data/{network}/registry.json", json!({ "get": op(
        "A third-party token list, tested against kascov's own index",
        "The checking is the feature and the names are the byproduct. Only the fetch is cached; \
         the comparison is redone per request against the live index, so a token that graduates \
         is reflected without waiting for the list's TTL to lapse.",
        json!([network_param()]),
        responses(&[("200", "The cross-checked list."), ("404", "Unknown network.")]),
    ) }));

    // --- share surface ----------------------------------------------------
    add("/og/{network}/{id}", json!({ "get": op(
        "The 1200x630 Open Graph card",
        "Rendered on demand (SVG → PNG, embedded fonts) — Facebook/X reject SVG og:images. `.png` \
         suffix accepted.",
        json!([network_param(), covenant_id_param("Covenant id, 64 hex characters.")]),
        responses(&[
            ("200", "image/png."),
            ("400", "Malformed id."),
            ("404", "Unknown network or covenant."),
        ]),
    ) }));

    add("/badge/{network}/{id}", json!({ "get": op(
        "A shields-style README badge",
        "Live status for one covenant, as SVG. `.svg` suffix accepted.",
        json!([network_param(), covenant_id_param("Covenant id, 64 hex characters.")]),
        responses(&[
            ("200", "image/svg+xml."),
            ("400", "Malformed id."),
            ("404", "Unknown network or covenant."),
        ]),
    ) }));

    add("/img/{network}/{id}", json!({ "get": op(
        "The token's art — served ONLY when it hashes to the chain's commitment",
        "The bytes at the deployer's claimed URL are fetched once (SSRF-guarded), and served only \
         when they hash to the sha256 committed in the genesis payload. Chain-pinned art can never \
         be swapped, so a verified copy is immutable in practice. Only formats the magic-byte \
         sniff identifies are served: png, jpeg, gif, webp.",
        json!([network_param(), covenant_id_param("Token (covenant) id, 64 hex characters.")]),
        responses(&[
            ("200", "The verified image."),
            ("404", "Unknown network/token, no committed hash, or bytes that don't match it — mismatches retry after a day."),
        ]),
    ) }));

    add("/listed-img/{network}/{id}", json!({ "get": op(
        "The witnessed copy of a listed logo",
        "kascov's own copy, deliberately outside the proven /img namespace — that one's cache \
         headers promise chain-proven bytes, and these are only witnessed.",
        json!([network_param(), covenant_id_param("Token (covenant) id, 64 hex characters.")]),
        responses(&[
            ("200", "The witnessed image."),
            ("404", "Unknown network/token or no witnessed copy."),
        ]),
    ) }));

    add("/data/{network}/index.json", json!({ "get": op(
        "The machine-readable front door",
        "Production 404 logs showed integrators guessing URLs on first contact; this document is \
         what a guessed URL should land near. Static per network, no DB touch.",
        json!([network_param()]),
        responses(&[("200", "Endpoint URLs, clients, and the open-data note."), ("404", "Unknown network.")]),
    ) }));

    add("/share/{network}/{id}", json!({ "get": op(
        "A crawler-visible share page",
        "~1KB shell with OG/Twitter meta for a covenant or transaction — the SPA is hash-routed, \
         so scrapers never see #/… urls.",
        json!([network_param(), param("id", "path", true, json!({ "type": "string" }),
            "Covenant id or transaction id, 64 hex characters.")]),
        responses(&[
            ("200", "text/html."),
            ("404", "Unknown network or id."),
        ]),
    ) }));

    add("/sitemap.xml", json!({ "get": op(
        "The sitemap",
        "The root plus the newest 5000 mainnet coins as /share URLs.",
        json!([]),
        responses(&[("200", "application/xml.")]),
    ) }));

    add("/feed.xml", json!({ "get": op(
        "The changelog as an Atom feed",
        "For readers and the crawlers that follow feeds.",
        json!([]),
        responses(&[("200", "application/atom+xml.")]),
    ) }));

    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "kascov worker API",
            // The same hash /healthz answers as `build`: the document is
            // versioned by the binary that serves it, nothing else.
            "version": env!("KASCOV_GIT_HASH"),
            "description": "Open JSON over the kascov index — no keys, CORS open. Every displayed fact is decodable from the chain's own revealed programs; anything unproven is served with an explicit reason (candles' `reason`, the book's `provenance`) or not at all.",
        },
        "servers": [{ "url": "https://kascov.io" }],
        "paths": Value::Object(paths),
        "components": { "schemas": {
            "RestingOrders": {
                "type": "array",
                "description": "One side of the book, best price first (bids highest, asks lowest), ordered on the exact price pair via i128 cross-multiplication — a float would collapse close price levels. Rows with a side or price shape that cannot order are dropped whole rather than guessed at.",
                "items": {
                    "type": "object",
                    "required": ["covenant_id", "price", "amount", "maker", "expiry_daa", "created_daa"],
                    "properties": {
                        "covenant_id": { "type": "string", "description": "The order program's own covenant id." },
                        "price": price_pair_schema(),
                        "amount": { "type": "integer", "description": "Tokens offered." },
                        "maker": { "type": "string", "description": "Maker pubkey, hex." },
                        "maker_address": { "type": "string", "description": "The maker's address form, when the pubkey renders to one." },
                        "expiry_daa": { "type": "integer" },
                        "created_daa": { "type": "integer" },
                    },
                },
            },
        } },
    })
}

fn compile_body_schema() -> Value {
    json!({
        "type": "object",
        "required": ["source"],
        "properties": {
            "source": { "type": "string", "description": "SilverScript source." },
            "args": { "type": "array", "items": { "type": "string" }, "description": "Constructor args; defaults to none." },
        },
    })
}

#[cfg(test)]
mod api_doc_tests {
    use super::*;

    /// Every (path, method) the router registers, parsed straight out of
    /// main.rs. The parser panics on any `.route(` shape it can't read —
    /// a silently skipped route would be exactly the drift this test exists
    /// to refuse.
    fn router_routes() -> Vec<(String, String)> {
        let src = include_str!("main.rs");
        let mut out = Vec::new();
        let mut rest = src;
        while let Some(pos) = rest.find(".route(") {
            rest = &rest[pos + ".route(".len()..];
            let t = rest.trim_start();
            assert!(
                t.starts_with('"'),
                "a route path must directly follow .route( — teach the parser the new shape"
            );
            let t = &t[1..];
            let end = t.find('"').expect("unterminated route path literal");
            let path = &t[..end];
            let mut after = t[end + 1..].trim_start();
            after = after
                .strip_prefix(',')
                .expect("a route path must be followed by its handler")
                .trim_start();
            while let Some(comment) = after.strip_prefix("//") {
                let nl = comment.find('\n').expect("unterminated comment");
                after = comment[nl..].trim_start();
            }
            let method = if after.starts_with("get(") {
                "get"
            } else if after.starts_with("post(") {
                "post"
            } else {
                panic!("route {path}: method is neither get( nor post( — teach the parser");
            };
            out.push((path.to_string(), method.to_string()));
            rest = after;
        }
        assert!(
            out.len() > 40,
            "found only {} routes — the parser lost the router",
            out.len()
        );
        out
    }

    fn documented_routes(doc: &Value) -> Vec<(String, String)> {
        doc["paths"]
            .as_object()
            .expect("paths must be an object")
            .iter()
            .flat_map(|(path, item)| {
                item.as_object()
                    .expect("path item must be an object")
                    .keys()
                    .map(move |method| (path.clone(), method.clone()))
            })
            .collect()
    }

    /// The audit's finding, mechanized: the document may neither fall behind
    /// the router (an undocumented route) nor run ahead of it (a documented
    /// route nothing serves). Exact set equality, both directions.
    #[test]
    fn the_document_matches_the_router_exactly() {
        let router: std::collections::BTreeSet<(String, String)> =
            router_routes().into_iter().collect();
        let doc = document();
        let documented: std::collections::BTreeSet<(String, String)> =
            documented_routes(&doc).into_iter().collect();
        let undocumented: Vec<_> = router.difference(&documented).collect();
        let phantom: Vec<_> = documented.difference(&router).collect();
        assert!(
            undocumented.is_empty(),
            "routes the document doesn't know: {undocumented:?}"
        );
        assert!(
            phantom.is_empty(),
            "documented routes the router doesn't serve: {phantom:?}"
        );
    }

    /// Structural floor for every operation: a summary, and responses whose
    /// entries all carry a description — an empty description is a documented
    /// route that documents nothing.
    #[test]
    fn every_operation_has_a_summary_and_described_responses() {
        let doc = document();
        for (path, item) in doc["paths"].as_object().unwrap() {
            for (method, op) in item.as_object().unwrap() {
                let summary = op["summary"].as_str().unwrap_or("");
                assert!(!summary.is_empty(), "{method} {path}: empty summary");
                let responses = op["responses"]
                    .as_object()
                    .unwrap_or_else(|| panic!("{method} {path}: no responses"));
                assert!(!responses.is_empty(), "{method} {path}: no responses");
                for (code, resp) in responses {
                    let d = resp["description"].as_str().unwrap_or("");
                    assert!(!d.is_empty(), "{method} {path} {code}: empty description");
                }
            }
        }
    }

    /// The documented bucket enum is exactly the handler's allowlist, and
    /// every documented label actually parses. (The reverse — the parser
    /// accepting an undocumented label — is covered by parse_bucket being an
    /// explicit three-arm match, tested in candle_tests.)
    #[test]
    fn candles_document_the_parsers_bucket_allowlist() {
        let doc = document();
        let params = doc["paths"]["/data/{network}/token/{id}/candles"]["get"]["parameters"]
            .as_array()
            .unwrap();
        let bucket = params
            .iter()
            .find(|p| p["name"] == "bucket")
            .expect("candles must document the bucket param");
        let labels: Vec<&str> = bucket["schema"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(labels, ["1h", "4h", "1d"]);
        for label in labels {
            assert!(
                crate::parse_bucket(label).is_some(),
                "{label} is documented but the handler refuses it"
            );
        }
    }

    /// The candle schema keeps the replay link: both txid fields present and
    /// required, so no future edit can quietly drop the tie to replayable
    /// transactions.
    #[test]
    fn candles_document_the_replay_linkable_txids() {
        let doc = document();
        let candle = &doc["paths"]["/data/{network}/token/{id}/candles"]["get"]["responses"]
            ["200"]["content"]["application/json"]["schema"]["properties"]["candles"]["items"];
        let required: Vec<&str> = candle["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        for field in ["first_txid", "last_txid"] {
            assert!(required.contains(&field), "candle items must require {field}");
        }
    }

    /// The book's provenance note must say what the data is NOT — decoded,
    /// not verified — in the document, not only in the served body.
    #[test]
    fn the_book_documents_decoded_not_verified() {
        let doc = document();
        let schema = &doc["paths"]["/data/{network}/token/{id}/book"]["get"]["responses"]["200"]
            ["content"]["application/json"]["schema"];
        let note = schema["properties"]["provenance"]["description"]
            .as_str()
            .unwrap();
        assert!(
            note.contains("decoded, not verified"),
            "the book's provenance description lost its core claim: {note}"
        );
        let required: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(required.contains(&"provenance"));
    }

    /// The matched enum names every search lane, "listed" included, and each
    /// value's provenance is explained in the description.
    #[test]
    fn search_documents_every_matched_lane() {
        let doc = document();
        let matched = &doc["paths"]["/data/{network}/search"]["get"]["responses"]["200"]
            ["content"]["application/json"]["schema"]["properties"]["results"]["items"]
            ["properties"]["matched"];
        let values: Vec<&str> = matched["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(values, ["id", "name", "claimed", "listed", "template"]);
        let description = matched["description"].as_str().unwrap();
        for value in values {
            assert!(
                description.contains(&format!("\"{value}\"")),
                "matched:\"{value}\" has no explained provenance"
            );
        }
    }

    /// /healthz documents the build field, and /health carries the identical
    /// contract (one handler serves both).
    #[test]
    fn healthz_documents_the_build_field() {
        let doc = document();
        for path in ["/healthz", "/health"] {
            let props = &doc["paths"][path]["get"]["responses"]["200"]["content"]
                ["application/json"]["schema"]["properties"];
            let build = props["build"]["description"].as_str().unwrap_or("");
            assert!(
                build.contains("git hash"),
                "{path} must document what `build` is: {build:?}"
            );
        }
    }

    /// The document versions itself by the binary that serves it.
    #[test]
    fn the_document_is_versioned_by_the_build() {
        let doc = document();
        assert_eq!(doc["openapi"], "3.1.0");
        assert_eq!(doc["info"]["version"], env!("KASCOV_GIT_HASH"));
        assert!(!doc["info"]["version"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_handler_serves_json_with_open_cors() {
        let resp = openapi_handler().await;
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let headers = resp.headers();
        assert_eq!(
            headers[axum::http::header::CONTENT_TYPE],
            "application/json; charset=utf-8"
        );
        assert_eq!(headers[axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
    }
}
