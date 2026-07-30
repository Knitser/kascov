//! Transaction preflight — "will this transaction pass?", answered BEFORE
//! broadcast, with the same primitives a node uses. Feed it the SDK-dict /
//! RPC JSON of a transaction and it reports the traps covenant builders
//! actually hit (rusty-kaspa#1073's missing computeBudget, the forgotten fee
//! input, the block-mass ceiling), computes the real consensus masses, and —
//! when inputs carry their witness and utxo — runs them through Kaspa's
//! actual script engine metered against each input's own committed budget.
//!
//! Pure computation: no node, no keys, no state. The handler in main.rs adds
//! the rate limiter and the body cap; everything here is testable offline.

use kaspa_consensus_core::config::params::{Params, MAINNET_PARAMS, TESTNET_PARAMS};
use kaspa_consensus_core::mass::units::{ComputeBudget, ScriptUnits};
use kaspa_consensus_core::mass::{calc_storage_mass, MassCalculator, UtxoCell};
use kaspa_consensus_core::subnets::SUBNETWORK_ID_NATIVE;
use kaspa_consensus_core::tx::{
    ComputeCommit, CovenantBinding, MutableTransaction, ScriptPublicKey, Transaction,
    TransactionInput, TransactionOutpoint, TransactionOutput, UtxoEntry,
};
use kascov_core::Network;

/// Consensus caps (Params::max_tx_inputs/max_tx_outputs) — anything past them
/// is un-relayable regardless of what the fields say.
const MAX_TX_IO: usize = 1000;
/// Ceiling on engine executions per request — execution burns real CPU and
/// the findings past the first dozen inputs repeat themselves anyway.
const EXEC_INPUT_CAP: usize = 16;

fn params_for(network: Network) -> &'static Params {
    match network {
        Network::Mainnet => &MAINNET_PARAMS,
        // testnet-10 params; other testnet suffixes share them.
        Network::Testnet(_) => &TESTNET_PARAMS,
    }
}

/// One diagnosis. `code` is the stable machine key the frontend switches on;
/// `message` lifts the guide's trap prose so the tool and the guide speak the
/// same language.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Finding {
    pub severity: &'static str, // "error" | "warn" | "info"
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

/// A parsed input — everything optional, because the whole point is telling
/// the caller what's missing instead of failing on it.
#[derive(Default)]
struct PInput {
    outpoint_txid: Option<[u8; 32]>,
    outpoint_index: u32,
    sequence: u64,
    sig_op_count: Option<u64>,
    /// The budget as the node will see it (camelCase field only — a
    /// snake_case `compute_budget` is what pre-Toccata SDK serializers drop,
    /// so it deliberately does NOT populate this).
    compute_budget: Option<u64>,
    signature_script: Option<Vec<u8>>,
    utxo_amount: Option<u64>,
    utxo_spk: Option<(u16, Vec<u8>)>,
    utxo_covenant_id: Option<[u8; 32]>,
}

#[derive(Default)]
struct POutput {
    value: Option<u64>,
    spk: Option<(u16, Vec<u8>)>,
    covenant: Option<([u8; 32], u16)>, // (covenant id, authorizing input)
}

/// Everything the checks need, pulled out of the JSON with findings recorded
/// along the way.
struct PTx {
    version: Option<u64>,
    inputs: Vec<PInput>,
    outputs: Vec<POutput>,
    lock_time: u64,
    payload: Vec<u8>,
}

// ── tolerant JSON access ───────────────────────────────────────────────────

/// camelCase → snake_case ("computeBudget" → "compute_budget").
fn snake_of(camel: &str) -> String {
    let mut out = String::with_capacity(camel.len() + 4);
    for c in camel.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Integer that may arrive as a JSON number or a decimal string (u64 doesn't
/// round-trip through every SDK's JSON layer, so strings are common).
fn as_u64(v: &serde_json::Value) -> Option<u64> {
    match v {
        serde_json::Value::Number(n) => n.as_u64(),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn as_hex(v: &serde_json::Value) -> Option<Vec<u8>> {
    let s = v.as_str()?.trim().trim_start_matches("0x");
    hex::decode(s).ok()
}

fn as_hex32(v: &serde_json::Value) -> Option<[u8; 32]> {
    as_hex(v)?.try_into().ok()
}

/// Field lookup: camelCase first, then the snake_case spelling. The caller
/// learns which spelling matched, so snake_case is DETECTED, never silently
/// normalized away.
fn field<'a>(obj: &'a serde_json::Map<String, serde_json::Value>, camel: &str) -> (Option<&'a serde_json::Value>, bool) {
    if let Some(v) = obj.get(camel) {
        return (Some(v), false);
    }
    let snake = snake_of(camel);
    if snake != camel {
        if let Some(v) = obj.get(&snake) {
            return (Some(v), true);
        }
    }
    (None, false)
}

/// Collector for parse-time findings.
struct ParseLog {
    findings: Vec<Finding>,
    /// snake_case fields already reported (one finding per field name).
    snake_seen: Vec<String>,
    /// unknown keys seen anywhere (deduped, order kept).
    unknown: Vec<String>,
}

impl ParseLog {
    fn new() -> Self {
        Self { findings: Vec::new(), snake_seen: Vec::new(), unknown: Vec::new() }
    }

    fn snake(&mut self, camel: &str, input_index: Option<usize>) {
        let name = snake_of(camel);
        if self.snake_seen.contains(&name) {
            return;
        }
        self.snake_seen.push(name.clone());
        // compute_budget is THE proven footgun (rusty-kaspa#1073 wears it):
        // SDKs pinned to pre-Toccata wire formats (wasm/py 2.0.x) silently
        // drop the snake_case field when serializing, the node sees budget 0
        // and answers limit=9999 no matter what the local object said.
        if name == "compute_budget" {
            self.findings.push(Finding {
                severity: "warn",
                code: "snake_case_field",
                message: "`compute_budget` is spelled snake_case — SDK serializers pinned to pre-Toccata wire \
                          formats silently drop the field, so the node sees budget 0 and answers `limit=9999` \
                          no matter what your local object said. This preflight mirrors the node and ignores \
                          it too; spell it `computeBudget` in Toccata-aware tooling."
                    .into(),
                input_index,
                suggestion: Some("rename compute_budget → computeBudget".into()),
            });
        } else {
            self.findings.push(Finding {
                severity: "info",
                code: "snake_case_field",
                message: format!(
                    "`{name}` is spelled snake_case — accepted here, but Toccata-era SDK dictionaries expect \
                     camelCase (`{camel}`); serializers frozen pre-Toccata may drop it"
                ),
                input_index,
                suggestion: None,
            });
        }
    }

    fn unknown_keys(&mut self, obj: &serde_json::Map<String, serde_json::Value>, known: &[&str]) {
        for key in obj.keys() {
            let matches_known =
                known.iter().any(|k| key.as_str() == *k || key.as_str() == snake_of(k));
            if !matches_known && !self.unknown.contains(key) {
                self.unknown.push(key.clone());
            }
        }
    }
}

fn parse_spk(v: &serde_json::Value, log: &mut ParseLog, input_index: Option<usize>) -> Option<(u16, Vec<u8>)> {
    match v {
        serde_json::Value::String(_) => as_hex(v).map(|script| (0u16, script)),
        serde_json::Value::Object(obj) => {
            log.unknown_keys(obj, &["version", "script", "scriptPublicKey", "hex"]);
            let (version, vsnake) = field(obj, "version");
            if vsnake {
                log.snake("version", input_index);
            }
            let version = version.and_then(as_u64).unwrap_or(0) as u16;
            let script = ["script", "scriptPublicKey", "hex"]
                .iter()
                .find_map(|k| {
                    let (v, snake) = field(obj, k);
                    if snake {
                        log.snake(k, input_index);
                    }
                    v.and_then(as_hex)
                })?;
            Some((version, script))
        }
        _ => None,
    }
}

/// Parse the submitted JSON into a PTx + parse findings. `Err` is reserved
/// for bodies we can't analyze at all (the handler's 400).
fn parse(body: &str, log: &mut ParseLog) -> Result<PTx, String> {
    let root: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("not valid JSON: {e}"))?;
    let mut obj = root.as_object().ok_or("expected a JSON object describing a transaction")?;
    // Tolerate the RPC submit wrapper: {"transaction": {...}, "allowOrphan": …}.
    if let Some(inner) = obj.get("transaction").and_then(|v| v.as_object()) {
        obj = inner;
    }
    log.unknown_keys(
        obj,
        &["version", "inputs", "outputs", "lockTime", "subnetworkId", "gas", "payload", "mass", "id", "transaction", "allowOrphan", "verboseData"],
    );

    let (version, snake) = field(obj, "version");
    if snake {
        log.snake("version", None);
    }
    let version = version.and_then(as_u64);

    let (lock_time, snake) = field(obj, "lockTime");
    if snake {
        log.snake("lockTime", None);
    }
    let lock_time = lock_time.and_then(as_u64).unwrap_or(0);

    let (payload, snake) = field(obj, "payload");
    if snake {
        log.snake("payload", None);
    }
    let payload = payload.and_then(as_hex).unwrap_or_default();

    let (inputs_v, snake) = field(obj, "inputs");
    if snake {
        log.snake("inputs", None);
    }
    let inputs_v = inputs_v.and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let (outputs_v, snake) = field(obj, "outputs");
    if snake {
        log.snake("outputs", None);
    }
    let outputs_v = outputs_v.and_then(|v| v.as_array()).cloned().unwrap_or_default();

    let mut inputs = Vec::with_capacity(inputs_v.len().min(MAX_TX_IO));
    for (i, iv) in inputs_v.iter().enumerate().take(MAX_TX_IO) {
        let mut input = PInput::default();
        let Some(iobj) = iv.as_object() else {
            inputs.push(input);
            continue;
        };
        log.unknown_keys(
            iobj,
            &["previousOutpoint", "sequence", "sigOpCount", "computeBudget", "signatureScript", "utxo", "utxoEntry", "verboseData"],
        );

        let (outpoint, snake) = field(iobj, "previousOutpoint");
        if snake {
            log.snake("previousOutpoint", Some(i));
        }
        if let Some(oobj) = outpoint.and_then(|v| v.as_object()) {
            log.unknown_keys(oobj, &["transactionId", "index"]);
            let (txid, snake) = field(oobj, "transactionId");
            if snake {
                log.snake("transactionId", Some(i));
            }
            input.outpoint_txid = txid.and_then(as_hex32);
            let (index, snake) = field(oobj, "index");
            if snake {
                log.snake("index", Some(i));
            }
            input.outpoint_index = index.and_then(as_u64).unwrap_or(0) as u32;
        }

        let (sequence, snake) = field(iobj, "sequence");
        if snake {
            log.snake("sequence", Some(i));
        }
        input.sequence = sequence.and_then(as_u64).unwrap_or(0);

        let (sigops, snake) = field(iobj, "sigOpCount");
        if snake {
            log.snake("sigOpCount", Some(i));
        }
        input.sig_op_count = sigops.and_then(as_u64);

        // camelCase ONLY populates the budget; snake_case is detected and
        // deliberately ignored — exactly what a pre-Toccata serializer does.
        if let Some(v) = iobj.get("computeBudget") {
            input.compute_budget = as_u64(v);
            if input.compute_budget.is_none() {
                log.findings.push(Finding {
                    severity: "warn",
                    code: "bad_number",
                    message: format!("input {i}: computeBudget isn't a readable integer — treating it as absent"),
                    input_index: Some(i),
                    suggestion: None,
                });
            }
        } else if iobj.contains_key("compute_budget") {
            log.snake("computeBudget", Some(i));
        }

        let (sig, snake) = field(iobj, "signatureScript");
        if snake {
            log.snake("signatureScript", Some(i));
        }
        if let Some(v) = sig {
            input.signature_script = as_hex(v);
            if input.signature_script.is_none() && !v.is_null() {
                log.findings.push(Finding {
                    severity: "warn",
                    code: "bad_hex",
                    message: format!("input {i}: signatureScript isn't valid hex — treating it as absent"),
                    input_index: Some(i),
                    suggestion: None,
                });
            }
        }

        let utxo = ["utxo", "utxoEntry"].iter().find_map(|k| {
            let (v, snake) = field(iobj, k);
            if snake {
                log.snake(k, Some(i));
            }
            v.and_then(|v| v.as_object())
        });
        if let Some(uobj) = utxo {
            log.unknown_keys(uobj, &["amount", "value", "scriptPublicKey", "blockDaaScore", "isCoinbase", "covenantId"]);
            input.utxo_amount = ["amount", "value"].iter().find_map(|k| {
                let (v, snake) = field(uobj, k);
                if snake {
                    log.snake(k, Some(i));
                }
                v.and_then(as_u64)
            });
            let (spk, snake) = field(uobj, "scriptPublicKey");
            if snake {
                log.snake("scriptPublicKey", Some(i));
            }
            input.utxo_spk = spk.and_then(|v| parse_spk(v, log, Some(i)));
            let (cov, snake) = field(uobj, "covenantId");
            if snake {
                log.snake("covenantId", Some(i));
            }
            input.utxo_covenant_id = cov.and_then(as_hex32);
        }
        inputs.push(input);
    }

    let mut outputs = Vec::with_capacity(outputs_v.len().min(MAX_TX_IO));
    for (i, ov) in outputs_v.iter().enumerate().take(MAX_TX_IO) {
        let mut output = POutput::default();
        let Some(oobj) = ov.as_object() else {
            outputs.push(output);
            continue;
        };
        log.unknown_keys(oobj, &["value", "amount", "scriptPublicKey", "covenant", "verboseData"]);
        output.value = ["value", "amount"].iter().find_map(|k| {
            let (v, snake) = field(oobj, k);
            if snake {
                log.snake(k, None);
            }
            v.and_then(as_u64)
        });
        let (spk, snake) = field(oobj, "scriptPublicKey");
        if snake {
            log.snake("scriptPublicKey", None);
        }
        output.spk = spk.and_then(|v| parse_spk(v, log, None));
        let (cov, snake) = field(oobj, "covenant");
        if snake {
            log.snake("covenant", None);
        }
        if let Some(cobj) = cov.and_then(|v| v.as_object()) {
            log.unknown_keys(cobj, &["covenantId", "id", "authorizingInput"]);
            let id = ["covenantId", "id"].iter().find_map(|k| {
                let (v, snake) = field(cobj, k);
                if snake {
                    log.snake(k, None);
                }
                v.and_then(as_hex32)
            });
            let (auth, snake) = field(cobj, "authorizingInput");
            if snake {
                log.snake("authorizingInput", None);
            }
            let auth = auth.and_then(as_u64).unwrap_or(0) as u16;
            output.covenant = id.map(|id| (id, auth));
        }
        let _ = i;
        outputs.push(output);
    }

    if inputs_v.len() > MAX_TX_IO || outputs_v.len() > MAX_TX_IO {
        log.findings.push(Finding {
            severity: "error",
            code: "too_many_io",
            message: format!(
                "{} inputs / {} outputs — consensus caps both at {MAX_TX_IO}; only the first {MAX_TX_IO} were analyzed",
                inputs_v.len(),
                outputs_v.len()
            ),
            input_index: None,
            suggestion: None,
        });
    }

    Ok(PTx { version, inputs, outputs, lock_time, payload })
}

// ── static checks ──────────────────────────────────────────────────────────

/// Does this input look like a covenant spend (as opposed to the plain
/// fee/change input everybody forgets)? P2SH-shaped utxo lock, or a witness
/// big enough to be revealing a program rather than pushing pk+signature.
fn covenant_looking(input: &PInput) -> bool {
    if let Some((_, script)) = &input.utxo_spk {
        if kascov_decode::p2sh_hash(script).is_some() {
            return true;
        }
    }
    input.signature_script.as_ref().is_some_and(|s| s.len() > 150)
}

/// The smallest computeBudget whose allowance covers `used` script units —
/// the consensus rounding (free allowance included), not naive division.
pub fn covering_budget(used: u64) -> Option<u16> {
    ComputeBudget::checked_covering_script_units(ScriptUnits(used)).map(|b| b.0)
}

/// Run the preflight over a request body. `Err` means the body wasn't
/// analyzable at all (not JSON / not an object) → the handler's 400.
pub fn run(body: &str, network: Network) -> Result<serde_json::Value, String> {
    let mut log = ParseLog::new();
    let ptx = parse(body, &mut log)?;
    let mut findings = std::mem::take(&mut log.findings);
    let params = params_for(network);

    if !log.unknown.is_empty() {
        let shown: Vec<&str> = log.unknown.iter().take(5).map(|s| s.as_str()).collect();
        findings.push(Finding {
            severity: "info",
            code: "unknown_fields",
            message: format!(
                "{} unrecognized field{} ignored ({}{})",
                log.unknown.len(),
                if log.unknown.len() == 1 { "" } else { "s" },
                shown.join(", "),
                if log.unknown.len() > 5 { ", …" } else { "" },
            ),
            input_index: None,
            suggestion: None,
        });
    }

    let version = match ptx.version {
        Some(v) => v,
        None => {
            findings.push(Finding {
                severity: "warn",
                code: "version_missing",
                message: "no version field — assuming version 1 (Toccata); pre-Toccata transactions must say version 0".into(),
                input_index: None,
                suggestion: Some("set version: 1".into()),
            });
            1
        }
    };
    let version_known = version <= 1;
    if !version_known {
        findings.push(Finding {
            severity: "warn",
            code: "version_unknown",
            message: format!("version {version} isn't a Kaspa transaction version this tool knows (0 = legacy, 1 = Toccata) — static checks were skipped"),
            input_index: None,
            suggestion: None,
        });
    }

    if ptx.inputs.is_empty() {
        findings.push(Finding {
            severity: "error",
            code: "no_inputs",
            message: "the transaction has no inputs — nothing to spend".into(),
            input_index: None,
            suggestion: None,
        });
    }
    if ptx.outputs.is_empty() {
        findings.push(Finding {
            severity: "error",
            code: "no_outputs",
            message: "the transaction has no outputs — a valid transaction pays somewhere".into(),
            input_index: None,
            suggestion: None,
        });
    }

    // Execution first (when possible): measured script units make the budget
    // suggestions exact instead of heuristic.
    let executable: Vec<usize> = ptx
        .inputs
        .iter()
        .enumerate()
        .filter(|(_, inp)| {
            inp.signature_script.as_ref().is_some_and(|s| !s.is_empty())
                && inp.utxo_spk.as_ref().is_some_and(|(_, s)| !s.is_empty())
                && inp.utxo_amount.is_some()
        })
        .map(|(i, _)| i)
        .take(EXEC_INPUT_CAP)
        .collect();

    // Full-transaction context needs a buildable tx: every input populated
    // with a utxo, and every input's compute commitment expressible.
    let all_populated = !ptx.inputs.is_empty()
        && ptx
            .inputs
            .iter()
            .all(|inp| inp.utxo_spk.is_some() && inp.utxo_amount.is_some());

    // Per-input budget traps (v1 only — v0 inputs commit sigOpCount).
    // `budgets_ok` = every v1 input's commitment is expressible on the wire,
    // the precondition for the mass calculator (it panics otherwise).
    let mut budgets_ok = true;
    if version == 1 {
        for (i, input) in ptx.inputs.iter().enumerate() {
            let budget = input.compute_budget;
            if budget.is_none() {
                budgets_ok = false;
            }
            if let Some(b) = budget {
                if b > u16::MAX as u64 {
                    budgets_ok = false;
                    findings.push(Finding {
                        severity: "error",
                        code: "budget_overflow",
                        message: format!("input {i}: computeBudget {b} doesn't fit the u16 wire field (max 65,535)"),
                        input_index: Some(i),
                        suggestion: None,
                    });
                    continue;
                }
                if input.sig_op_count.is_some_and(|s| s > 0) {
                    findings.push(Finding {
                        severity: "info",
                        code: "sigop_count_ignored",
                        message: format!(
                            "input {i}: sigOpCount isn't part of the version-1 wire format — serializers drop it; the committed computeBudget is what counts"
                        ),
                        input_index: Some(i),
                        suggestion: None,
                    });
                }
            }
            if budget.is_none() && input.sig_op_count.is_some_and(|s| s > 0) {
                // The migration finding: a v0-shaped input pasted into a v1 tx.
                findings.push(Finding {
                    severity: "error",
                    code: "sigop_count_on_v1",
                    message: format!(
                        "input {i} still carries the v0 per-input sigOpCount — version-1 (Toccata) transactions replace it with a computeBudget: u16 commitment (1 budget unit = 100 grams = 10,000 script units). Without it the node grants only the free 9,999 units."
                    ),
                    input_index: Some(i),
                    suggestion: Some("replace sigOpCount with computeBudget (10 covers one CheckSig)".into()),
                });
                continue;
            }
            if budget.is_none() || budget == Some(0) {
                // Evidence beats heuristic: if this input executes cleanly
                // under budget 0, the free 9,999 units genuinely cover it.
                let measured = executable.contains(&i).then(|| {
                    let (spk_v, spk) = input.utxo_spk.clone().expect("executable implies utxo");
                    kascov_sim::preflight_execute_isolated(
                        i,
                        spk_v,
                        &spk,
                        input.signature_script.as_deref().unwrap_or_default(),
                        input.utxo_amount.unwrap_or(0),
                        u16::MAX,
                        input.utxo_covenant_id,
                    )
                });
                if let Some(probe) = &measured {
                    if probe.pass && probe.script_units_used <= 9_999 {
                        continue; // budget 0 honestly suffices for this input
                    }
                }
                let suggestion = match &measured {
                    Some(probe) if probe.pass => covering_budget(probe.script_units_used)
                        .map(|b| format!("set computeBudget: {} — this witness measured {} script units", b.max(1), probe.script_units_used)),
                    _ if covenant_looking(input) => Some(
                        "set computeBudget: 20 (kascov-lab's default per input), or measure the exact need with a dry-run".into(),
                    ),
                    _ => Some("set computeBudget: 10 — one Schnorr CheckSig costs exactly 100,000 script units (10 units)".into()),
                };
                if covenant_looking(input) {
                    findings.push(Finding {
                        severity: "error",
                        code: "budget_missing",
                        message: format!(
                            "input {i} commits {} computeBudget — its effective allowance is the free 9,999 script units, and one Schnorr CheckSig alone costs 100,000 (the `used=100000, limit=9999` of rusty-kaspa#1073). The budget is committed by the spending input and covered by the signature hash — set it BEFORE signing; it can't be patched in afterwards.",
                            if budget.is_none() { "no" } else { "a zero" },
                        ),
                        input_index: Some(i),
                        suggestion,
                    });
                } else {
                    findings.push(Finding {
                        severity: "error",
                        code: "fee_input_budget_missing",
                        message: format!(
                            "input {i} looks like the plain fee/change input — the one everybody forgets because it \"never needed anything\" pre-Toccata. EVERY input of a version-1 transaction commits its own budget, and this one's CheckSig needs ≥ 10.",
                        ),
                        input_index: Some(i),
                        suggestion,
                    });
                }
            }
        }
    } else if version == 0 {
        for (i, input) in ptx.inputs.iter().enumerate() {
            if input.compute_budget.is_some() {
                findings.push(Finding {
                    severity: "warn",
                    code: "budget_on_v0",
                    message: format!(
                        "input {i} sets computeBudget on a version-0 transaction — v0 inputs commit sigOpCount, so the field is ignored on the wire. Covenant (Toccata) features need version: 1."
                    ),
                    input_index: Some(i),
                    suggestion: Some("set version: 1 to commit compute budgets".into()),
                });
            }
            if input.sig_op_count.is_some_and(|s| s > u8::MAX as u64) {
                findings.push(Finding {
                    severity: "error",
                    code: "sigop_overflow",
                    message: format!("input {i}: sigOpCount {} doesn't fit the u8 wire field", input.sig_op_count.unwrap_or(0)),
                    input_index: Some(i),
                    suggestion: None,
                });
            }
        }
    }

    // Output sanity (needed for both consensus and the storage-mass math).
    let mut outputs_ok = true;
    for (i, output) in ptx.outputs.iter().enumerate() {
        if output.value.is_none() || output.value == Some(0) {
            outputs_ok = false;
            findings.push(Finding {
                severity: "error",
                code: "output_value_zero",
                message: format!("output {i} has no (or zero) value — Kaspa outputs must carry a positive amount (KIP-9 storage mass divides by it)"),
                input_index: None,
                suggestion: None,
            });
        }
        if output.spk.as_ref().is_none_or(|(_, s)| s.is_empty()) {
            outputs_ok = false;
            findings.push(Finding {
                severity: "error",
                code: "output_spk_missing",
                message: format!("output {i} has no scriptPublicKey — nothing would be able to spend it, and the node rejects it"),
                input_index: None,
                suggestion: None,
            });
        }
    }

    // ── consensus masses ──────────────────────────────────────────────────
    // GUARD: MassCalculator::calc_non_contextual_masses panics on a v1 input
    // without a budget commitment ("v1 transactions are expected to have
    // compute budget"), so it only runs when every commitment is expressible.
    let mut masses_json = None;
    let mut fee_json = None;
    let mut masses_under_limit = false;
    let buildable = version_known
        && !ptx.inputs.is_empty()
        && !ptx.outputs.is_empty()
        && (version == 0 || budgets_ok);
    let tx = buildable.then(|| build_tx(&ptx, version as u16));
    if let Some(tx) = &tx {
        let calc = MassCalculator::new_with_consensus_params(params);
        let nc = calc.calc_non_contextual_masses(tx);
        let limits = params.block_mass_limits().after();
        let mut over = Vec::new();
        if nc.compute_mass > limits.compute {
            over.push(format!("compute {} > {}", nc.compute_mass, limits.compute));
        }
        if nc.transient_mass > limits.transient {
            over.push(format!("transient {} > {}", nc.transient_mass, limits.transient));
        }

        // Storage mass needs every input amount (KIP-9 reads both sides).
        let mut storage: Option<u64> = None;
        let amounts_known = ptx.inputs.iter().all(|i| i.utxo_amount.is_some_and(|a| a > 0));
        if amounts_known && outputs_ok {
            let input_cells: Vec<UtxoCell> = ptx
                .inputs
                .iter()
                .map(|i| match &i.utxo_spk {
                    Some((v, s)) => (&UtxoEntry::new(
                        i.utxo_amount.unwrap_or(1),
                        ScriptPublicKey::from_vec(*v, s.clone()),
                        0,
                        false,
                        None,
                    ))
                        .into(),
                    None => UtxoCell::new(1, i.utxo_amount.unwrap_or(1)),
                })
                .collect();
            let output_cells = tx.outputs.iter().map(UtxoCell::from);
            match calc_storage_mass(false, input_cells.iter().copied(), output_cells, params.storage_mass_parameter) {
                Some(mass) => {
                    if mass > limits.storage {
                        over.push(format!("storage {} > {}", mass, limits.storage));
                    }
                    storage = Some(mass);
                }
                None => {
                    findings.push(Finding {
                        severity: "error",
                        code: "storage_mass_incomputable",
                        message: "storage mass overflows — the outputs split the value too finely for the inputs (KIP-9); consolidate outputs or raise their amounts".into(),
                        input_index: None,
                        suggestion: None,
                    });
                }
            }
        } else if outputs_ok {
            findings.push(Finding {
                severity: "info",
                code: "storage_mass_skipped",
                message: "storage mass wasn't checked — include each input's utxo amount to check the KIP-9 side too".into(),
                input_index: None,
                suggestion: None,
            });
        }

        if !over.is_empty() {
            findings.push(Finding {
                severity: "error",
                code: "mass_exceeds_limit",
                message: format!(
                    "mass exceeds the per-block ceiling ({}) — the transaction is un-spendable on-chain; no fee fixes it. Commit smaller budgets or split the transaction.",
                    over.join(", ")
                ),
                input_index: None,
                suggestion: None,
            });
        }
        masses_under_limit = over.is_empty();
        masses_json = Some(serde_json::json!({
            "compute": nc.compute_mass,
            "transient": nc.transient_mass,
            "storage": storage,
            "limit": { "compute": limits.compute, "transient": limits.transient, "storage": limits.storage },
        }));
        // The fee family kascov-lab ships for its spends (100 sompi per gram
        // of compute mass + fixed headroom) — labeled an estimate because
        // that's what it is: Kaspa has no consensus minimum-fee primitive.
        fee_json = Some(serde_json::json!({
            "estimate_sompi": 100u64.saturating_mul(nc.compute_mass).saturating_add(200_000),
            "note": "estimate from kascov-lab's shipped fee formula (100 sompi × compute grams + 200,000 headroom) — Kaspa has no consensus minimum fee; mempools set their own floors",
        }));
    } else if version_known && !ptx.inputs.is_empty() && !ptx.outputs.is_empty() {
        findings.push(Finding {
            severity: "info",
            code: "mass_skipped",
            message: "consensus masses weren't computed — every input of a version-1 transaction must commit a computeBudget first (see the findings above)".into(),
            input_index: None,
            suggestion: None,
        });
    }

    // ── engine execution ──────────────────────────────────────────────────
    let mut executed: Vec<kascov_sim::InputExec> = Vec::new();
    let mut execution_note = None;
    if !executable.is_empty() {
        if let (Some(tx), true) = (&tx, all_populated) {
            // Real context: the engine sees the declared outputs, sequences
            // and covenant bindings, so introspection AND signatures are
            // faithful — what a node validates, minus chain context.
            let entries: Vec<UtxoEntry> = ptx
                .inputs
                .iter()
                .map(|i| {
                    let (v, s) = i.utxo_spk.clone().expect("all_populated");
                    UtxoEntry::new(
                        i.utxo_amount.expect("all_populated"),
                        ScriptPublicKey::from_vec(v, s),
                        0,
                        false,
                        i.utxo_covenant_id.map(kaspa_consensus_core::Hash::from_bytes),
                    )
                })
                .collect();
            let mtx = MutableTransaction::with_entries(tx.clone(), entries);
            executed = kascov_sim::preflight_execute(&mtx, &executable);
            execution_note = Some(
                "executed against the transaction as submitted — outputs, sequences and covenant bindings are the real ones, so signature and introspection checks are faithful",
            );
        } else {
            // Isolated fallback: each input replayed in a fabricated
            // 1-in/1-out continuation (same context the tx debugger uses).
            for &i in &executable {
                let input = &ptx.inputs[i];
                let (spk_v, spk) = input.utxo_spk.clone().expect("executable implies utxo");
                executed.push(kascov_sim::preflight_execute_isolated(
                    i,
                    spk_v,
                    &spk,
                    input.signature_script.as_deref().unwrap_or_default(),
                    input.utxo_amount.unwrap_or(0),
                    input.compute_budget.unwrap_or(0).min(u16::MAX as u64) as u16,
                    input.utxo_covenant_id,
                ));
            }
            execution_note = Some(
                "executed in an isolated per-input context (not every input carried a utxo, so the full transaction couldn't be populated) — signature and introspection checks may diverge from a real validation",
            );
        }
        for exec in &executed {
            if !exec.pass {
                let units_exceeded = exec.verdict.contains("script units exceeded");
                findings.push(Finding {
                    severity: "error",
                    code: "input_script_failed",
                    message: format!("input {}: the script engine rejected the witness — {}", exec.input_index, exec.verdict),
                    input_index: Some(exec.input_index),
                    suggestion: units_exceeded
                        .then(|| "raise this input's computeBudget to cover its measured script units".to_string()),
                });
            }
        }
    }

    // ── verdict ───────────────────────────────────────────────────────────
    let has_error = findings.iter().any(|f| f.severity == "error");
    let verdict = if has_error {
        "will_fail"
    } else if masses_json.is_some() && masses_under_limit {
        "ready"
    } else {
        "incomplete"
    };

    Ok(serde_json::json!({
        "ok": true,
        "network": network.to_string(),
        "verdict": verdict,
        "findings": findings,
        "masses": masses_json,
        "fee": fee_json,
        "executed": if executed.is_empty() { None } else { Some(&executed) },
        "execution_note": execution_note,
        "note": "static + engine preflight over the transaction as submitted — chain context (utxo existence, maturity, fee market) still belongs to the node",
    }))
}

/// Build the consensus Transaction the mass calculator and the engine share.
/// Only called when `buildable` holds (v1 inputs all carry budgets).
fn build_tx(ptx: &PTx, version: u16) -> Transaction {
    let inputs = ptx
        .inputs
        .iter()
        .map(|i| {
            let commit = if version >= 1 {
                ComputeCommit::ComputeBudget(ComputeBudget(i.compute_budget.unwrap_or(0).min(u16::MAX as u64) as u16))
            } else {
                // Plain spends carry one CheckSig; assume it when unstated.
                ComputeCommit::SigopCount((i.sig_op_count.unwrap_or(1).min(u8::MAX as u64) as u8).into())
            };
            TransactionInput {
                previous_outpoint: TransactionOutpoint::new(
                    kaspa_consensus_core::Hash::from_bytes(i.outpoint_txid.unwrap_or([0; 32])),
                    i.outpoint_index,
                ),
                signature_script: i.signature_script.clone().unwrap_or_default(),
                sequence: i.sequence,
                compute_commit: commit,
            }
        })
        .collect();
    let outputs = ptx
        .outputs
        .iter()
        .map(|o| {
            TransactionOutput::with_covenant(
                o.value.unwrap_or(1),
                o.spk
                    .clone()
                    .map(|(v, s)| ScriptPublicKey::from_vec(v, s))
                    .unwrap_or_else(|| ScriptPublicKey::from_vec(0, Vec::new())),
                o.covenant
                    .map(|(id, auth)| CovenantBinding::new(auth, kaspa_consensus_core::Hash::from_bytes(id))),
            )
        })
        .collect();
    Transaction::new(version, inputs, outputs, ptx.lock_time, SUBNETWORK_ID_NATIVE, 0, ptx.payload.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TN10: Network = Network::Testnet(10);

    /// P2SH lock of `program` (OpBlake2b <32-byte hash> OpEqual, version 0) —
    /// the same shape kaspa_txscript::pay_to_script_hash_script builds.
    fn p2sh_spk(program: &[u8]) -> Vec<u8> {
        let hash = blake2b_simd::Params::new().hash_length(32).hash(program);
        let mut spk = vec![0xaa, 0x20];
        spk.extend_from_slice(hash.as_bytes());
        spk.push(0x87);
        spk
    }

    fn run_tn10(body: &str) -> serde_json::Value {
        run(body, TN10).expect("analyzable body")
    }

    fn codes(v: &serde_json::Value) -> Vec<String> {
        v["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["code"].as_str().unwrap().to_string())
            .collect()
    }

    fn finding<'a>(v: &'a serde_json::Value, code: &str) -> &'a serde_json::Value {
        v["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["code"] == code)
            .unwrap_or_else(|| panic!("expected finding {code}, got {:?}", codes(v)))
    }

    /// A well-formed v1 transaction: both inputs budgeted, amounts known.
    fn good_v1() -> String {
        serde_json::json!({
            "version": 1,
            "inputs": [
                {
                    "previousOutpoint": { "transactionId": "11".repeat(32), "index": 0 },
                    "sequence": 0,
                    "computeBudget": 20,
                    "utxo": { "amount": 1_000_000_000u64, "scriptPublicKey": { "version": 0, "script": "aa20".to_string() + &"22".repeat(32) + "87" } }
                },
                {
                    "previousOutpoint": { "transactionId": "33".repeat(32), "index": 1 },
                    "sequence": 0,
                    "computeBudget": 10,
                    "utxo": { "amount": 500_000_000u64, "scriptPublicKey": { "version": 0, "script": "20".to_string() + &"44".repeat(32) + "ac" } }
                }
            ],
            "outputs": [
                { "value": 999_999_000u64, "scriptPublicKey": { "version": 0, "script": "20".to_string() + &"55".repeat(32) + "ac" } },
                { "value": 499_000_000u64, "scriptPublicKey": { "version": 0, "script": "20".to_string() + &"66".repeat(32) + "ac" } }
            ],
            "lockTime": 0
        })
        .to_string()
    }

    // ── parse fixtures ─────────────────────────────────────────────────

    #[test]
    fn camel_case_parses_clean_and_ready() {
        let v = run_tn10(&good_v1());
        assert_eq!(v["ok"], true);
        assert_eq!(v["verdict"], "ready", "findings: {:?}", codes(&v));
        assert!(v["masses"]["compute"].as_u64().unwrap() > 0);
        assert!(v["masses"]["storage"].as_u64().is_some());
        assert!(v["fee"]["estimate_sompi"].as_u64().unwrap() > 0);
        assert!(!codes(&v).contains(&"snake_case_field".to_string()));
    }

    #[test]
    fn snake_case_compute_budget_is_detected_not_silently_accepted() {
        let body = good_v1().replace("\"computeBudget\":20", "\"compute_budget\":20");
        let v = run_tn10(&body);
        let snake = finding(&v, "snake_case_field");
        assert_eq!(snake["severity"], "warn");
        assert!(snake["message"].as_str().unwrap().contains("limit=9999"));
        // …and the node-visible consequence fires too: that input has budget 0.
        assert!(codes(&v).contains(&"budget_missing".to_string()));
        assert_eq!(v["verdict"], "will_fail");
    }

    #[test]
    fn string_ints_are_tolerated() {
        let body = serde_json::json!({
            "version": "1",
            "inputs": [{ "computeBudget": "20", "sequence": "0",
                         "utxo": { "amount": "1000000000", "scriptPublicKey": { "version": "0", "script": "20".to_string() + &"44".repeat(32) + "ac" } } }],
            "outputs": [{ "value": "999999000", "scriptPublicKey": "20".to_string() + &"55".repeat(32) + "ac" }],
        })
        .to_string();
        let v = run_tn10(&body);
        assert_eq!(v["verdict"], "ready", "findings: {:?}", codes(&v));
        assert_eq!(v["masses"]["storage"].as_u64().is_some(), true);
    }

    #[test]
    fn garbage_is_a_clean_error_not_a_panic() {
        assert!(run("not json at all {{{", TN10).is_err());
        assert!(run("[1,2,3]", TN10).is_err());
        assert!(run("\"just a string\"", TN10).is_err());
        // an object with nothing useful still analyzes (as incomplete)
        let v = run_tn10("{}");
        assert_eq!(v["verdict"], "will_fail"); // no inputs, no outputs
        assert!(codes(&v).contains(&"no_inputs".to_string()));
        assert!(codes(&v).contains(&"no_outputs".to_string()));
    }

    #[test]
    fn rpc_submit_wrapper_is_unwrapped() {
        let body = format!("{{\"transaction\": {}, \"allowOrphan\": false}}", good_v1());
        let v = run_tn10(&body);
        assert_eq!(v["verdict"], "ready", "findings: {:?}", codes(&v));
    }

    #[test]
    fn unknown_fields_are_counted_not_rejected() {
        let body = good_v1().replacen("\"version\":1", "\"version\":1,\"frobnicator\":7", 1);
        let v = run_tn10(&body);
        assert!(finding(&v, "unknown_fields")["message"].as_str().unwrap().contains("frobnicator"));
        assert_eq!(v["verdict"], "ready");
    }

    // ── one test per trap ──────────────────────────────────────────────

    #[test]
    fn trap_missing_budget_on_covenant_input() {
        let body = good_v1().replacen("\"computeBudget\":20,", "", 1);
        let v = run_tn10(&body);
        let f = finding(&v, "budget_missing");
        assert_eq!(f["severity"], "error");
        assert_eq!(f["input_index"], 0);
        let msg = f["message"].as_str().unwrap();
        assert!(msg.contains("9,999") && msg.contains("100,000"), "trap numbers must be spelled out: {msg}");
        assert!(f["suggestion"].as_str().unwrap().contains("computeBudget"));
        assert_eq!(v["verdict"], "will_fail");
        // the guard: masses must be absent, not a panic
        assert!(v["masses"].is_null());
        assert!(codes(&v).contains(&"mass_skipped".to_string()));
    }

    #[test]
    fn trap_fee_input_budget_reminder() {
        // input 1 is the plain p2pk fee input; drop only ITS budget
        let body = good_v1().replacen("\"computeBudget\":10,", "", 1);
        let v = run_tn10(&body);
        let f = finding(&v, "fee_input_budget_missing");
        assert_eq!(f["input_index"], 1);
        assert!(f["message"].as_str().unwrap().contains("fee/change input"));
        assert!(f["suggestion"].as_str().unwrap().contains("computeBudget: 10"));
        assert_eq!(v["verdict"], "will_fail");
    }

    #[test]
    fn trap_budget_zero_fires_too() {
        let body = good_v1().replacen("\"computeBudget\":20", "\"computeBudget\":0", 1);
        let v = run_tn10(&body);
        assert_eq!(finding(&v, "budget_missing")["input_index"], 0);
        // zero budgets ARE expressible, so masses still compute (no panic path)
        assert!(v["masses"]["compute"].as_u64().unwrap() > 0);
        assert_eq!(v["verdict"], "will_fail");
    }

    #[test]
    fn trap_budget_on_v0_is_flagged_ignored() {
        let body = good_v1().replacen("\"version\":1", "\"version\":0", 1);
        let v = run_tn10(&body);
        let f = finding(&v, "budget_on_v0");
        assert_eq!(f["severity"], "warn");
        assert!(f["suggestion"].as_str().unwrap().contains("version: 1"));
    }

    #[test]
    fn trap_sigop_count_on_v1_is_the_migration_finding() {
        let body = good_v1().replacen("\"computeBudget\":20,", "\"sigOpCount\":1,", 1);
        let v = run_tn10(&body);
        let f = finding(&v, "sigop_count_on_v1");
        assert_eq!(f["severity"], "error");
        assert!(f["message"].as_str().unwrap().contains("10,000 script units"));
        assert_eq!(v["verdict"], "will_fail");
    }

    #[test]
    fn trap_mass_ceiling_makes_it_unspendable() {
        // 65,535 budget units = 6,553,500 grams of compute mass — 13× the
        // 500,000 block ceiling. Committed mass is charged whether the script
        // uses it or not (the guide's "commit generously, but not 65535").
        let body = good_v1().replacen("\"computeBudget\":20", "\"computeBudget\":65535", 1);
        let v = run_tn10(&body);
        let f = finding(&v, "mass_exceeds_limit");
        assert!(f["message"].as_str().unwrap().contains("un-spendable"));
        assert!(v["masses"]["compute"].as_u64().unwrap() > 500_000);
        assert_eq!(v["verdict"], "will_fail");
    }

    #[test]
    fn trap_zero_value_output() {
        let body = good_v1().replacen("\"value\":999999000", "\"value\":0", 1);
        let v = run_tn10(&body);
        assert_eq!(finding(&v, "output_value_zero")["severity"], "error");
        assert_eq!(v["verdict"], "will_fail");
    }

    #[test]
    fn storage_mass_needs_input_amounts() {
        let body = good_v1().replace("\"amount\":1000000000,", "").replace("\"amount\":500000000,", "");
        let v = run_tn10(&body);
        assert!(codes(&v).contains(&"storage_mass_skipped".to_string()));
        assert!(v["masses"]["storage"].is_null());
        // compute mass still computed and clean → ready
        assert_eq!(v["verdict"], "ready", "findings: {:?}", codes(&v));
    }

    // ── covering-budget suggestion math ────────────────────────────────

    #[test]
    fn covering_budget_respects_the_free_allowance_boundaries() {
        assert_eq!(covering_budget(0), Some(0));
        assert_eq!(covering_budget(9_999), Some(0)); // the free allowance
        assert_eq!(covering_budget(10_000), Some(1));
        assert_eq!(covering_budget(19_999), Some(1));
        assert_eq!(covering_budget(20_000), Some(2));
        assert_eq!(covering_budget(100_000), Some(10)); // one CheckSig
        assert_eq!(covering_budget(u64::MAX), None); // beyond u16 budgets
    }

    // ── engine execution ───────────────────────────────────────────────

    /// A synthetic-but-real execution: OpTrue behind a P2SH commitment, the
    /// exact reveal shape covenants use. Full tx context (utxo on every
    /// input) → the engine sees the declared outputs.
    #[test]
    fn execution_passes_a_clean_p2sh_reveal() {
        let program = vec![0x51u8]; // OpTrue
        let spk = p2sh_spk(&program);
        let witness = kascov_decode::encode_push(&program);
        let body = serde_json::json!({
            "version": 1,
            "inputs": [{
                "previousOutpoint": { "transactionId": "11".repeat(32), "index": 0 },
                "sequence": 0,
                "computeBudget": 1,
                "signatureScript": hex::encode(&witness),
                "utxo": { "amount": 100_000_000u64, "scriptPublicKey": { "version": 0, "script": hex::encode(&spk) } }
            }],
            "outputs": [{ "value": 99_999_000u64, "scriptPublicKey": { "version": 0, "script": "20".to_string() + &"55".repeat(32) + "ac" } }],
        })
        .to_string();
        let v = run_tn10(&body);
        let exec = &v["executed"][0];
        assert_eq!(exec["pass"], true, "verdict: {}", exec["verdict"]);
        assert_eq!(exec["input_index"], 0);
        assert_eq!(exec["allowance"], 19_999); // budget 1 × 10,000 + 9,999 free
        assert!(exec["script_units_used"].as_u64().unwrap() <= 19_999);
        assert_eq!(v["verdict"], "ready", "findings: {:?}", codes(&v));
        assert!(v["execution_note"].as_str().unwrap().contains("transaction as submitted"));
    }

    /// The REAL bytes of an accepted testnet-10 covenant spend (a terminal
    /// SilverScript · Mecenas `receive`: output-constrained, no signature —
    /// exactly the witness class whose validity survives replay). Captured
    /// from the production index; refresh with the ignored helper below if
    /// testnet-10 resets.
    mod real_witness {
        /// P2SH lock of the state coin (spk version 0).
        pub const STATE_SPK: &str = "aa20693bc1d2d058eae1ca60ed0050f8fbcd724652f6a3e51b7976d8bb4d480231f887";
        /// The on-chain unlocking script of the spend.
        pub const WITNESS: &str = "004cb56b6c76009c6375025802b100c320366db7e0f3350cfd60638e0c631061d8d8fac72600ee5e7e258d1fc939b28675030000207c7e01ac7e876902e803b9be760480f0fa0294527994760480f0fa02547993a16300c252795479949c696700c20480f0fa029c6951c3b9bf876951c2789c6968007a75007a75007a75516776519c637578aa20e3777a271a8e60c379317a67b9e5978d82542ed6805192b9558be8e1006649fe8769765279ac69757551677500696868";
        /// The state coin's value in sompi.
        pub const VALUE: u64 = 99_998_000;
        /// The spending input's committed budget (as accepted on-chain).
        pub const BUDGET: u16 = 20;
        /// The input sequence the spend carried (the contract's age gate
        /// compiles to OpCheckSequenceVerify over this field).
        pub const SEQUENCE: u64 = 600;
        /// Where the money went: recipient p2pk, value − the contract's 1000.
        pub const OUT_SPK: &str = "20366db7e0f3350cfd60638e0c631061d8d8fac72600ee5e7e258d1fc939b28675ac";
        pub const OUT_VALUE: u64 = 99_997_000;
    }

    #[test]
    fn execution_passes_a_real_captured_witness() {
        let body = serde_json::json!({
            "version": 1,
            "inputs": [{
                "previousOutpoint": { "transactionId": "11".repeat(32), "index": 0 },
                "sequence": real_witness::SEQUENCE,
                "computeBudget": real_witness::BUDGET,
                "signatureScript": real_witness::WITNESS,
                "utxo": { "amount": real_witness::VALUE, "scriptPublicKey": { "version": 0, "script": real_witness::STATE_SPK } }
            }],
            "outputs": [{ "value": real_witness::OUT_VALUE, "scriptPublicKey": { "version": 0, "script": real_witness::OUT_SPK } }],
        })
        .to_string();
        let v = run_tn10(&body);
        let exec = &v["executed"][0];
        assert_eq!(exec["pass"], true, "the real accepted witness must replay clean: {}", exec["verdict"]);
        let used = exec["script_units_used"].as_u64().unwrap();
        assert!(used > 0 && used <= exec["allowance"].as_u64().unwrap());
        assert_eq!(v["verdict"], "ready", "findings: {:?}", codes(&v));
    }

    #[test]
    fn execution_failure_is_a_finding_and_fails_the_verdict() {
        // Reveal a program that doesn't match the P2SH commitment.
        let spk = p2sh_spk(&[0x51]);
        let witness = kascov_decode::encode_push(&[0x52]); // wrong program
        let body = serde_json::json!({
            "version": 1,
            "inputs": [{
                "computeBudget": 1,
                "signatureScript": hex::encode(&witness),
                "utxo": { "amount": 100_000_000u64, "scriptPublicKey": { "version": 0, "script": hex::encode(&spk) } }
            }],
            "outputs": [{ "value": 99_999_000u64, "scriptPublicKey": { "version": 0, "script": "20".to_string() + &"55".repeat(32) + "ac" } }],
        })
        .to_string();
        let v = run_tn10(&body);
        assert_eq!(v["executed"][0]["pass"], false);
        assert!(codes(&v).contains(&"input_script_failed".to_string()));
        assert_eq!(v["verdict"], "will_fail");
    }

    /// Fixture refresher: walks a testnet-10 index copy for a terminal
    /// Mecenas `receive` whose replay passes, and prints the constants for
    /// `real_witness` above. Run by hand when testnet-10 resets:
    /// `KASCOV_FIXTURE_DB=/path/to/testnet-10.db cargo test -p kascov extract_real_witness_fixture -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn extract_real_witness_fixture() {
        let Ok(db) = std::env::var("KASCOV_FIXTURE_DB") else {
            eprintln!("set KASCOV_FIXTURE_DB to a testnet-10 index copy");
            return;
        };
        let store = kascov_core::store::Store::open(std::path::Path::new(&db), TN10).unwrap();
        let registry = kascov_decode::Registry::default();
        for summary in store.covenants_with_templates(&["SilverScript · Mecenas"]).unwrap() {
            for utxo in store.utxos(&summary.covenant_id, false).unwrap() {
                let Some(sig) = utxo.spent_sig.clone().filter(|s| !s.is_empty()) else { continue };
                let spk = utxo.spk_script.clone();
                let Some(program) = kascov_decode::p2sh_reveal(&spk, &sig) else { continue };
                let decoded = registry.decode(0, &program);
                let field = |n: &str| decoded.fields.iter().find(|f| f.name == n).map(|f| f.value.clone());
                let (Some(recipient), Some(pledge), Some(period)) = (field("recipient"), field("pledge"), field("period")) else {
                    continue;
                };
                let pledge = i64::from_le_bytes({
                    let mut b = [0u8; 8];
                    b[..pledge.len().min(8)].copy_from_slice(&pledge[..pledge.len().min(8)]);
                    b
                }) as u64;
                let period = i64::from_le_bytes({
                    let mut b = [0u8; 8];
                    b[..period.len().min(8)].copy_from_slice(&period[..period.len().min(8)]);
                    b
                }) as u64;
                // terminal receive: everything − 1000 to the recipient
                if utxo.value as i128 - pledge as i128 - 1000 > pledge as i128 + 1000 {
                    continue;
                }
                let mut out_spk = vec![0x20];
                out_spk.extend_from_slice(&recipient);
                out_spk.push(0xac);
                let exec = kascov_sim::preflight_execute(
                    &{
                        let ptx = PTx {
                            version: Some(1),
                            inputs: vec![PInput {
                                sequence: period,
                                compute_budget: Some(utxo.spent_budget.unwrap_or(20) as u64),
                                signature_script: Some(sig.clone()),
                                utxo_amount: Some(utxo.value),
                                utxo_spk: Some((utxo.spk_version, spk.clone())),
                                ..Default::default()
                            }],
                            outputs: vec![POutput {
                                value: Some(utxo.value - 1000),
                                spk: Some((0, out_spk.clone())),
                                covenant: None,
                            }],
                            lock_time: 0,
                            payload: Vec::new(),
                        };
                        let tx = build_tx(&ptx, 1);
                        let entries = vec![UtxoEntry::new(
                            utxo.value,
                            ScriptPublicKey::from_vec(utxo.spk_version, spk.clone()),
                            0,
                            false,
                            None,
                        )];
                        MutableTransaction::with_entries(tx, entries)
                    },
                    &[0],
                );
                if exec[0].pass {
                    println!("STATE_SPK: {}", hex::encode(&spk));
                    println!("WITNESS:   {}", hex::encode(&sig));
                    println!("VALUE:     {}", utxo.value);
                    println!("BUDGET:    {}", utxo.spent_budget.unwrap_or(20));
                    println!("SEQUENCE:  {period}");
                    println!("OUT_SPK:   {}", hex::encode(&out_spk));
                    println!("OUT_VALUE: {}", utxo.value - 1000);
                    return;
                }
                eprintln!("candidate {} failed: {}", summary.covenant_id, exec[0].verdict);
            }
        }
        panic!("no passing terminal Mecenas receive found in {db}");
    }
}
