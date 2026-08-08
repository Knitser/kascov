/* Live curve state for the $KASCOV trade page, read from kascov's own API and
 * the Kaspa node — no dependence on KRON's site or SDK.
 *
 * Every figure here is either a chain fact kascov re-derived (the program hash,
 * the curve's own economic constants, the tracked reserve) or a live node
 * reading (the curve's single spendable UTXO). Reads fail loud: on any error
 * this surfaces it rather than inventing a reserve, a price, or an outpoint. A
 * transaction built against stale state is not a loss — SIGHASH_ALL commits the
 * exact outpoint, so a trade that landed first simply invalidates a pending one
 * and it is rebuilt from the new tip — but a FABRICATED state could sign away
 * real money, so nothing is guessed. */

/* $KASCOV, fully known and pinned. The market covenant id is deliberately NOT
 * hardcoded: it is read from the market endpoint, so a repin of the curve build
 * does not silently point the page at a dead covenant. */
import { nextCurveProgram, p2shScriptPubKey } from './curve.js';

export const KASCOV_TOKEN_ID =
  'c58c826d0aa9cee62f93208718c674883f5c89a8aca4933dc41fb0391539abe2';

/* The layout the page's tx builder was written against, proven byte-for-byte
 * from real $KASCOV trades. If the live market matched a different build, the
 * BUY/SELL push layout differs and the builder must refuse — getCurveState
 * flags the mismatch rather than letting a trade be built blind. */
export const SUPPORTED_SKELETON = 'KRON curve v1';

/* The curve's own constants, baked into its hash-committed bytecode. Exposed
 * for the builder to compute outputs against — but getCurveState reads the
 * SAME values back from the API's verified program row and warns on any
 * disagreement, so these are a cross-check, never the source of truth. */
export const CURVE_CONSTANTS = Object.freeze({
  SCALE_SOMPI: 1_000_000, // kasIn/kasOut move in whole multiples of this (0.01 KAS)
  V_KAS_UNITS: 6_250_000, // IDX_VKAS; V in sompi = V_KAS_UNITS * SCALE_SOMPI
  GRADUATION_KAS_SOMPI: 25_000_000_000_000,
  FEE_CREATOR_BPS: 25,
  FEE_PLATFORM_BPS: 90,
  FEE_DEV_BPS: 10,
  FEE_DEV_FLOOR_SOMPI: 20_000_000, // 0.2 KAS
});

const DATA_BASE = '/data'; // kascov API, root-absolute so any page path resolves it
const KASPA_API = 'https://api.kaspa.org';
const SDK_URL = new URL('./sdk/web/kaspa.js', import.meta.url);
const SDK_WASM_URL = new URL('./sdk/web/kaspa_bg.wasm', import.meta.url);

const HEX32 = /^[0-9a-f]{64}$/;

function normalizeHex32(raw, label) {
  const hex = String(raw || '')
    .trim()
    .toLowerCase()
    .replace(/^0x/, '');
  if (!HEX32.test(hex)) throw new Error(`${label} must be 32 hex-encoded bytes`);
  return hex;
}

/* --------------------------- curve state --------------------------------- */

/* The live curve state a trade is built against:
 *   - the market covenant, its verified program hash and matched build;
 *   - the curve's own economic constants (V, graduation target), read back;
 *   - the tracked reserve and graduation progress (display; the indexed
 *     token_reserve is one spend behind the live cell and is labelled so);
 *   - the P2SH script the curve pays to;
 *   - the curve's SINGLE live UTXO (outpoint + value), which a trade must spend.
 *
 * `warnings` collects every reason the caller should hesitate: a build the page
 * does not recognise, a market that is no longer bonding, a failed invariant,
 * or a live UTXO the page could not read. A trade must not be built while
 * `warnings` is non-empty or `liveUtxo.outpoint` is absent.
 *
 * opts: { network='mainnet', dataBase='/data', getAddressUtxos(addr)?,
 *         deriveAddress(scriptHex, network)? } — the last two let a caller
 * supply its own node/address plumbing and skip the WASM SDK entirely. */
export async function getCurveState(opts = {}) {
  const network = opts.network || 'mainnet';
  const base = opts.dataBase || DATA_BASE;
  const warnings = [];

  const res = await fetch(`${base}/${network}/token/${KASCOV_TOKEN_ID}/market`, {
    cache: 'no-cache',
    headers: { accept: 'application/json' },
  });
  if (res.status === 404) {
    throw new Error('kascov has no verified market for $KASCOV on this network');
  }
  if (!res.ok) throw new Error(`market read failed: HTTP ${res.status}`);
  const payload = await res.json();

  const program = payload.program;
  if (!program || !program.covenant_id || !program.program_hash) {
    throw new Error('market payload is missing its verified program row');
  }
  const summary = payload.market || {};

  const marketCovenantId = normalizeHex32(program.covenant_id, 'market covenant id');
  const programHash = normalizeHex32(program.program_hash, 'program hash');

  /* The market must name OUR token, or it is a different market entirely. */
  if (program.token_covenant_id) {
    const named = normalizeHex32(program.token_covenant_id, 'program token id');
    if (named !== KASCOV_TOKEN_ID) {
      throw new Error(`market names token ${named}, not $KASCOV`);
    }
  }

  const skeleton = String(program.skeleton || '');
  const skeletonSupported = skeleton === SUPPORTED_SKELETON;
  if (!skeletonSupported) {
    warnings.push(
      `live market build is "${skeleton || 'unknown'}"; the page's trade builder targets ` +
        `${SUPPORTED_SKELETON}. Do not build a trade against an unrecognised layout.`,
    );
  }

  const phase = summary.phase || null;
  const bonding = phase === 'bonding';
  if (!bonding) {
    warnings.push(`market phase is "${phase || 'unknown'}", not bonding — the curve may no longer sell.`);
  }

  const invariantOk = program.invariant_ok === true;
  if (!invariantOk) warnings.push('the market program has not passed its constant-product replay.');

  /* Read the curve's own constants back and cross-check the public copies. */
  const vKasUnits = program.v_kas_units;
  const vSompi = vKasUnits != null ? BigInt(vKasUnits) * BigInt(CURVE_CONSTANTS.SCALE_SOMPI) : null;
  if (vKasUnits != null && Number(vKasUnits) !== CURVE_CONSTANTS.V_KAS_UNITS) {
    warnings.push(
      `on-chain vKas (${vKasUnits}) differs from the pinned constant ` +
        `(${CURVE_CONSTANTS.V_KAS_UNITS}) — economics changed; do not trust cached constants.`,
    );
  }
  const graduationKasSompi = program.graduation_kas_sompi ?? null;

  /* The buildable live cell, from kascov's own index in ONE fast same-origin
   * call: outpoint, value, committed script, and the base program to reveal.
   * The page then reconstructs the live program itself and refuses to trade
   * unless its blake2b matches the on-chain committed script — kascov serves
   * the ingredients, this page verifies the result. */
  let liveUtxo = { outpoint: null, error: 'not read' };
  let programHex = null;
  let liveReserve = null;
  let curveValueSompi = null;
  try {
    const cell = await fetchJson(`${base}/${network}/token/${KASCOV_TOKEN_ID}/curve-cell`);
    if (cell.live_count != null && cell.live_count !== 1) {
      warnings.push(`the curve shows ${cell.live_count} live cells — it may be mid-trade; a build will refuse.`);
    }
    const [txid, idx] = String(cell.outpoint || '').split(':');
    liveUtxo = {
      source: 'index',
      outpoint: txid && idx != null ? { transactionId: txid, index: Number(idx) } : null,
      valueSompi: cell.value_sompi != null ? String(cell.value_sompi) : null,
      scriptPublicKey: { version: 0, scriptPublicKey: cell.script_hex },
      count: cell.live_count ?? null,
    };
    curveValueSompi = cell.value_sompi != null ? String(cell.value_sompi) : null;
    liveReserve = cell.live_reserve != null ? String(cell.live_reserve) : null;
    /* reconstruct the live program and PROVE it against the committed script */
    if (cell.base_program_hex && cell.live_reserve != null && cell.script_hex) {
      const candidate = nextCurveProgram(cell.base_program_hex, BigInt(cell.live_reserve));
      const derivedScript = p2shScriptPubKey(candidate);
      if (derivedScript.toLowerCase() !== String(cell.script_hex).toLowerCase()) {
        /* never hand the builder a program that does not hash to the live
         * cell: a wrong reveal would build a transaction the covenant rejects,
         * or worse. Fail closed. */
        programHex = null;
        warnings.push('reconstructed curve program did not match the live cell hash; not buildable right now (retry).');
      } else {
        programHex = candidate;
      }
    }
  } catch (e) {
    warnings.push(`could not read the live curve cell: ${e && e.message ? e.message : e}`);
  }

  return {
    /* what curve.js quote/build read directly */
    reserve: liveReserve,
    value: curveValueSompi,
    programHex,
    marketXOnly: marketCovenantId,

    network,
    tokenId: KASCOV_TOKEN_ID,
    marketCovenantId,
    programHash,
    skeleton,
    skeletonSupported,
    phase,
    bonding,
    invariantOk,
    exercisedTrades: program.exercised_trades ?? null,

    /* economics (chain-derived) */
    vKasUnits: vKasUnits ?? null,
    vSompi: vSompi != null ? vSompi.toString() : null,
    graduationKasSompi,
    /* one spend behind the live cell — labelled, never called current */
    tokenReserveIndexed: program.token_reserve ?? null,

    /* display (indexer view) */
    reserveSompi: summary.reserve_sompi ?? null,
    gradProgressBps: summary.grad_progress_bps ?? null,
    spot:
      summary.spot_num_sompi != null && summary.spot_den != null
        ? { numSompi: String(summary.spot_num_sompi), den: String(summary.spot_den) }
        : null,
    lastQuoteSompi: summary.last_quote_sompi ?? null,
    lastBaseAmount: summary.last_base_amount ?? null,

    /* what a trade spends */
    p2shScript: `aa20${programHash}87`,
    liveUtxo,

    warnings,
    generatedAtMs: payload.generated_at_ms ?? null,
    /* the untouched verified rows, for the builder */
    raw: { market: summary, program },
  };
}

/* The curve is a single P2SH UTXO. Authoritative source is the live node
 * (queried by the curve's derived P2SH address); if that is unreachable this
 * falls back to kascov's own covenant-detail view of the live cell, flagged
 * `stalePossible` because the indexer can trail the tip by a block. Returns an
 * object carrying either { outpoint, valueSompi, ... } or { error }. */
async function getCurveLiveUtxo({ programHash, marketCovenantId, network, base, opts }) {
  const scriptHex = `aa20${programHash}87`;
  const nodeErrors = [];

  /* primary: the live node, by address */
  try {
    const address = await p2shAddressFromScript(scriptHex, network, opts);
    const rows = await fetchAddressUtxos(address, opts);
    const live = rows.filter((u) => u.outpoint && u.valueSompi != null);
    if (live.length >= 1) {
      /* the curve is one cell; if the node shows more, take none blindly —
       * surface the count and let the caller stop */
      const one = live.length === 1 ? live[0] : live.reduce((a, b) => (BigInt(a.valueSompi) >= BigInt(b.valueSompi) ? a : b));
      return {
        source: 'node',
        address,
        outpoint: one.outpoint,
        valueSompi: one.valueSompi,
        scriptPublicKey: one.scriptPublicKey || { version: 0, scriptPublicKey: scriptHex },
        count: live.length,
      };
    }
    nodeErrors.push('node reported no live UTXO at the curve address');
  } catch (e) {
    nodeErrors.push(`node path: ${e && e.message ? e.message : e}`);
  }

  /* fallback: kascov's own index (same-origin), which knows the live outpoint */
  try {
    const detail = await fetchJson(`${base}/${network}/c/${marketCovenantId}.json`);
    const utxos = Array.isArray(detail.utxos) ? detail.utxos : [];
    const live = utxos.filter((u) => u.live);
    if (live.length >= 1) {
      const one =
        live.length === 1
          ? live[0]
          : live.reduce((a, b) => (Number(a.value) >= Number(b.value) ? a : b));
      const [transactionId, index] = String(one.outpoint || '').split(':');
      if (!transactionId || index == null) throw new Error('index returned an unparseable outpoint');
      return {
        source: 'index',
        stalePossible: true,
        outpoint: { transactionId, index: Number(index) },
        valueSompi: String(one.value),
        scriptPublicKey: one.script_hex ? { version: 0, scriptPublicKey: one.script_hex } : { version: 0, scriptPublicKey: scriptHex },
        count: live.length,
      };
    }
    nodeErrors.push('index shows no live curve UTXO');
  } catch (e) {
    nodeErrors.push(`index path: ${e && e.message ? e.message : e}`);
  }

  return { source: null, outpoint: null, error: nodeErrors.join(' | ') };
}

/* --------------------------- user holdings ------------------------------- */

/* What a pubkey holds of $KASCOV, scanned from kascov's hash-proven holder
 * pages. A BUY does not need this — the buyer receives a fresh cell — but the
 * balance is worth showing and a SELL needs to know the cells exist.
 *
 * A KNOWN LIMITATION, stated plainly: kascov's API exposes per-owner BALANCE
 * and cell COUNT, not the spendable cell OUTPOINTS a SELL must reference. A
 * buyer's cell is owned as identifier type 0x03 ("presence:<pubkey>"); its P2SH
 * address changes with its committed amount, so a plain address lookup cannot
 * find it. Enumerating spendable cells therefore needs either a dedicated
 * holdings-by-pubkey endpoint (TODO on the worker) or reconstructing each
 * cell's committed program to derive its address and querying the node — work
 * the page's tx builder owns, not this reader. So `spendableCells` is null and
 * the caller is told why.
 *
 * The holder scan is bounded (opts.maxPages, default 40 pages of 500). Not
 * finding a pubkey within the bound is reported as `found:false, exhausted`,
 * which is NOT a claim of zero balance. */
/* One page of a token's live KCC-20 cells from kascov's own index, already
 * hash-proven server-side: each cell's reconstructed program blake2b-matches
 * the script the chain committed to, or the worker omits it. `owner` is the
 * 66-hex identifier the covenant stores, identifier_type first. */
async function fetchCells(ownerHex, opts = {}) {
  const network = opts.network || 'mainnet';
  const base = opts.dataBase || DATA_BASE;
  const url = `${base}/${network}/token/${KASCOV_TOKEN_ID}/cells?owner=${ownerHex}`;
  const page = await fetchJson(url); // throws on any read failure, never a silent zero
  const rows = Array.isArray(page.cells) ? page.cells : [];
  return {
    cells: rows.map((c) => ({
      /* the endpoint prints an outpoint as "txid:index"; a builder wants it
         structured, and keeping both means neither side has to re-parse */
      outpoint: splitOutpoint(c.outpoint),
      outpointText: c.outpoint,
      valueSompi: c.value_sompi,
      programHex: c.program_hex,
      scriptPublicKey: c.script_hex,
      amount: c.amount,
      owner: c.owner,
      identifierType: c.identifier_type,
    })),
    omittedUnproven: page.omitted_unproven ?? 0,
    omittedOverLimit: page.omitted_over_limit ?? 0,
    provenance: page.provenance ?? null,
  };
}

/* The market's inventory cell: the KCC-20 cell the curve covenant owns and
 * respends on every trade. Owner identifier is type 0x02 (covenant id) followed
 * by the market covenant. Exactly one cell is expected; anything else means the
 * market is mid-trade or the page is looking at a state it should not build on. */
export async function getInventoryCell(marketCovenantId, opts = {}) {
  const market = normalizeHex32(marketCovenantId, 'market covenant id');
  const { cells, omittedUnproven } = await fetchCells(`02${market}`, opts);
  if (cells.length !== 1) {
    throw new Error(
      `expected exactly one inventory cell, kascov returned ${cells.length}` +
        (omittedUnproven ? ` (${omittedUnproven} omitted as unproven)` : ''),
    );
  }
  return cells[0];
}

/* The trader's own token cells. A bought cell is presence-owned (type 0x03),
 * which is what a sell spends. Returns the real spendable cells, newest value
 * first, so a caller can pick one whole cell — the only sell shape any observed
 * trade proves. */
export async function getUserTokenCells(pubkey, opts = {}) {
  const xonly = normalizeHex32(pubkey, 'pubkey');
  const { cells, omittedUnproven, omittedOverLimit } = await fetchCells(`03${xonly}`, opts);
  const sorted = cells.slice().sort((a, b) => Number(BigInt(b.amount) - BigInt(a.amount)));
  const balance = sorted.reduce((s, c) => s + BigInt(c.amount), 0n);
  return {
    pubkey: xonly,
    cells: sorted,
    count: sorted.length,
    balance: balance.toString(),
    omittedUnproven,
    omittedOverLimit,
    /* every cell here is spendable: outpoint, value and a program whose hash
       the worker already checked against the chain's own commitment */
    spendable: true,
  };
}

function splitOutpoint(text) {
  const [txid, index] = String(text || '').split(':');
  if (!/^[0-9a-f]{64}$/i.test(txid || '') || index == null || index === '') {
    throw new Error(`unreadable outpoint from the cells endpoint: ${text}`);
  }
  return { transactionId: String(txid).toLowerCase(), index: Number(index) };
}

/* The user's change output has to pay their own address, and only the SDK
 * knows the canonical address→script encoding. The builder stays pure and asks
 * for this rather than importing the SDK itself. Returns the script hex. */
export async function scriptForAddress(address, network = 'mainnet') {
  if (!address) throw new Error('scriptForAddress needs an address');
  const kaspa = await loadSdk();
  const spk = kaspa.payToAddressScript(String(address));
  const hex = spk && typeof spk.toString === 'function' ? spk.toString() : null;
  const script = hex || (spk && spk.script) || null;
  if (!script) throw new Error(`the SDK could not encode ${address} on ${network}`);
  return String(script);
}

/* ------------------------------- plumbing -------------------------------- */

async function fetchJson(url) {
  const res = await fetch(url, { cache: 'no-cache', headers: { accept: 'application/json' } });
  if (!res.ok) throw new Error(`read failed: HTTP ${res.status} for ${url}`);
  return res.json();
}

/* api.kaspa.org address UTXOs, normalised to { outpoint, valueSompi }. A
 * caller may inject opts.getAddressUtxos(address) to use its own node/RPC. */
async function fetchAddressUtxos(address, opts) {
  if (opts && typeof opts.getAddressUtxos === 'function') {
    const rows = await opts.getAddressUtxos(address);
    return (rows || []).map(normalizeNodeUtxo);
  }
  const res = await fetch(`${KASPA_API}/addresses/${encodeURIComponent(address)}/utxos`, {
    headers: { accept: 'application/json' },
  });
  if (!res.ok) throw new Error(`api.kaspa.org UTXO read failed: HTTP ${res.status}`);
  const rows = await res.json();
  if (!Array.isArray(rows)) throw new Error('api.kaspa.org returned a non-array UTXO set');
  return rows.map(normalizeNodeUtxo);
}

function normalizeNodeUtxo(u) {
  const entry = u.utxoEntry || u.utxo || u;
  const op = u.outpoint || entry.outpoint || {};
  const transactionId = op.transactionId ?? op.transaction_id;
  const index = op.index;
  const amount = entry.amount ?? entry.value;
  if (transactionId == null || index == null || amount == null) {
    throw new Error('node UTXO entry missing outpoint or amount');
  }
  return {
    outpoint: { transactionId: String(transactionId), index: Number(index) },
    valueSompi: String(amount),
    scriptPublicKey: entry.scriptPublicKey || null,
  };
}

/* Derive the curve's Kaspa P2SH address from its P2SH scriptPubKey. Uses the
 * vendored WASM SDK (the canonical encoder); a caller may inject
 * opts.deriveAddress(scriptHex, network) to avoid loading it. */
async function p2shAddressFromScript(scriptHex, network, opts) {
  if (opts && typeof opts.deriveAddress === 'function') {
    const a = await opts.deriveAddress(scriptHex, network);
    if (!a) throw new Error('injected deriveAddress returned nothing');
    return String(a);
  }
  const kaspa = await loadSdk();
  const spk = new kaspa.ScriptPublicKey(0, scriptHex);
  const address = kaspa.addressFromScriptPublicKey(spk, network);
  if (!address) throw new Error('SDK could not derive an address from the curve script');
  return address.toString();
}

let sdkPromise = null;
function loadSdk() {
  if (!sdkPromise) {
    sdkPromise = import(SDK_URL.href).then(async (kaspa) => {
      await kaspa.default(SDK_WASM_URL.href);
      return kaspa;
    });
  }
  return sdkPromise;
}
