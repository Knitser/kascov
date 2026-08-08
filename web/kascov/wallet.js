/* KasWare adapter for the $KASCOV trade page.
 *
 * Real money rides on two invariants this file refuses to break:
 *   1. the wallet is asked to sign the user's funding inputs and NOTHING else;
 *   2. a signed transaction is broadcast only after proving the wallet changed
 *      nothing but those inputs' signatureScripts.
 * The page builds every covenant sigscript itself (the curve spend carries no
 * signature); if the wallet touched a covenant input, or any output, or the
 * outpoint of a funding input, the tx is discarded unbroadcast. Every read
 * fails loud — nothing here fabricates a UTXO, an address, or a success.
 *
 * This module owns the wallet handshake, funding-input signing and submission.
 * It does NOT build the trade transaction: that lives in the page's tx builder,
 * against the layout proven from real $KASCOV trades. */

/* api.kaspa.org is CORS-open to kascov.io (verified) and is the primary submit
 * and UTXO-read path. The vendored WASM SDK is the wRPC fallback. */
const KASPA_API = 'https://api.kaspa.org';
const SDK_URL = new URL('./sdk/web/kaspa.js', import.meta.url);
const SDK_WASM_URL = new URL('./sdk/web/kaspa_bg.wasm', import.meta.url);

/* SIGHASH_ALL: the curve commits its exact outpoint, so the user's funding
 * inputs must sign over the whole transaction — any other flag would let an
 * output be swapped after signing. */
export const SIGHASH_ALL = 1;

/* The one shape a funding input's signatureScript may take after signing:
 * a 0x41 push (65 bytes) = 64-byte Schnorr signature + 1 SIGHASH_ALL byte.
 * Anything else means the wallet did something other than a plain P2PK sign. */
export const FUNDING_SIG_RE = /^41[0-9a-f]{130}$/;

/* ------------------------------- wallet ---------------------------------- */

/* KasWare is the one wallet whose PSKT signing shape is documented and pinned
 * by the trade reconstruction; no other injected wallet is exercised. */
function getWallet() {
  const ks = typeof window !== 'undefined' ? window.kasware : undefined;
  if (!ks || typeof ks.signPskt !== 'function') return null;
  return ks;
}

/* Present AND carrying the trade surface this page needs. Extensions inject
 * late, so a caller that gets false should re-check after a beat rather than
 * conclude the user has no wallet. */
export function isInstalled() {
  const ks = getWallet();
  return Boolean(
    ks &&
      typeof ks.requestAccounts === 'function' &&
      typeof ks.getPublicKey === 'function' &&
      typeof ks.signPskt === 'function',
  );
}

function requireWallet() {
  const ks = getWallet();
  if (!ks) {
    throw new Error('KasWare not detected. Install it from kasware.xyz, then reload.');
  }
  return ks;
}

/* Compressed 33-byte key -> x-only 32-byte key (drop the 02/03 parity byte).
 * The covenant carries the buyer as an x-only pubkey, so this is the form the
 * tx builder needs. An already-x-only key is accepted as-is. */
function normalizePubkey(raw) {
  const hex = String(raw || '')
    .trim()
    .toLowerCase()
    .replace(/^0x/, '');
  if (!/^[0-9a-f]+$/.test(hex)) throw new Error('wallet returned a non-hex public key');
  if (hex.length === 66) {
    if (hex[0] !== '0' || (hex[1] !== '2' && hex[1] !== '3')) {
      throw new Error('wallet returned a 33-byte key without a 02/03 parity prefix');
    }
    return { xOnlyPubkey: hex.slice(2), compressedPubkey: hex };
  }
  if (hex.length === 64) return { xOnlyPubkey: hex, compressedPubkey: null };
  throw new Error(`wallet returned a ${hex.length / 2}-byte public key; expected 32 or 33`);
}

/* Connect and read the identity the page needs: the funding address and the
 * x-only pubkey that names the buyer in the covenant. */
export async function connect() {
  const ks = requireWallet();
  const accounts = await ks.requestAccounts();
  const address = Array.isArray(accounts) && accounts.length ? String(accounts[0]) : '';
  if (!address) throw new Error('the wallet reported no accounts');
  const { xOnlyPubkey, compressedPubkey } = normalizePubkey(await ks.getPublicKey());
  return { address, xOnlyPubkey, compressedPubkey };
}

/* --------------------------- funding UTXOs ------------------------------- */

/* Normalise one UTXO from KasWare's getUtxoEntries or the REST API into the
 * shape the tx builder consumes. Both the node's REST form
 * ({ outpoint, utxoEntry:{ amount, scriptPublicKey } }) and a flatter wallet
 * form are accepted; amounts are kept as strings (sompi can exceed 2^53). */
function normalizeUtxo(u) {
  if (!u || typeof u !== 'object') throw new Error('malformed UTXO entry');
  const entry = u.utxoEntry || u.utxo || u;
  const op = u.outpoint || entry.outpoint || {};
  const transactionId = op.transactionId ?? op.transaction_id ?? u.transactionId;
  const index = op.index ?? u.index;
  const amount = entry.amount ?? entry.value ?? u.amount ?? u.value;
  const spk = entry.scriptPublicKey ?? u.scriptPublicKey ?? null;
  if (transactionId == null || index == null || amount == null) {
    throw new Error('UTXO entry missing outpoint or amount');
  }
  return {
    outpoint: { transactionId: String(transactionId), index: Number(index) },
    amountSompi: String(amount),
    scriptPublicKey: spk,
    blockDaaScore: entry.blockDaaScore != null ? String(entry.blockDaaScore) : undefined,
    isCoinbase: Boolean(entry.isCoinbase),
  };
}

/* The user's spendable KAS, for the builder to select funding inputs from.
 * KasWare's own getUtxoEntries is preferred; api.kaspa.org is the fallback.
 * A caller may inject opts.getAddressUtxos(address) to use its own node. */
export async function getFundingUtxos(address, opts = {}) {
  if (!address) throw new Error('getFundingUtxos needs the funding address');
  if (typeof opts.getAddressUtxos === 'function') {
    const rows = await opts.getAddressUtxos(address);
    return (rows || []).map(normalizeUtxo);
  }
  const ks = getWallet();
  if (ks && typeof ks.getUtxoEntries === 'function') {
    try {
      const rows = await ks.getUtxoEntries(address);
      if (Array.isArray(rows) && rows.length) return rows.map(normalizeUtxo);
    } catch {
      /* The wallet answers this from its OWN node socket, which drops often
         enough that a dead socket must not mean "you have no funds". Fall
         through to the public read — the UTXO set is public either way, and
         nothing here is signed with what it returns. */
    }
  }
  return fetchAddressUtxosFromApi(address);
}

async function fetchAddressUtxosFromApi(address) {
  const res = await fetch(`${KASPA_API}/addresses/${encodeURIComponent(address)}/utxos`, {
    headers: { accept: 'application/json' },
  });
  if (!res.ok) throw new Error(`api.kaspa.org UTXO read failed: HTTP ${res.status}`);
  const rows = await res.json();
  if (!Array.isArray(rows)) throw new Error('api.kaspa.org returned a non-array UTXO set');
  return rows.map(normalizeUtxo);
}

/* ------------------------------ signing ---------------------------------- */

/* Order-insensitive structural equality. JSON carries no bigint, so `===` is
 * total on the leaf types a serialized transaction contains. */
function deepEqual(a, b) {
  if (a === b) return true;
  if (a === null || b === null || typeof a !== 'object' || typeof b !== 'object') return false;
  const aArr = Array.isArray(a);
  if (aArr !== Array.isArray(b)) return false;
  if (aArr) {
    if (a.length !== b.length) return false;
    for (let i = 0; i < a.length; i += 1) if (!deepEqual(a[i], b[i])) return false;
    return true;
  }
  const ak = Object.keys(a);
  const bk = Object.keys(b);
  if (ak.length !== bk.length) return false;
  for (const k of ak) {
    if (!Object.prototype.hasOwnProperty.call(b, k)) return false;
    if (!deepEqual(a[k], b[k])) return false;
  }
  return true;
}

/* A shallow clone with one key removed — for "everything but the signature is
 * identical" comparisons. */
function without(obj, key) {
  const out = {};
  for (const k of Object.keys(obj)) if (k !== key) out[k] = obj[k];
  return out;
}

/* THE SAFETY RAIL. Prove the wallet changed only what it was asked to, by
 * comparing the transaction before and after signing:
 *   - the input count is unchanged;
 *   - every non-funding input (each covenant input, sigscript included) is
 *     byte-identical;
 *   - every funding input changed ONLY its signatureScript, to a lone P2PK
 *     Schnorr push over SIGHASH_ALL;
 *   - every output and every other top-level field is byte-identical.
 * Throws on the first deviation. Never returns a mutated transaction. */
export function assertIntentPreserved(before, after, fundingIndexes) {
  if (!before || !after || !Array.isArray(before.inputs) || !Array.isArray(after.inputs)) {
    throw new Error('cannot verify signing: transaction has no inputs array');
  }
  if (before.inputs.length !== after.inputs.length) {
    throw new Error(
      `wallet changed the input count (${before.inputs.length} -> ${after.inputs.length})`,
    );
  }
  /* Everything that is not the inputs — outputs, version, lockTime,
   * subnetworkId, gas, payload — must survive byte for byte. */
  if (!deepEqual(without(before, 'inputs'), without(after, 'inputs'))) {
    throw new Error('wallet changed the outputs or a transaction-level field');
  }
  const funding = new Set(fundingIndexes.map(Number));
  for (let i = 0; i < before.inputs.length; i += 1) {
    const b = before.inputs[i];
    const a = after.inputs[i];
    if (funding.has(i)) {
      const sig = String(a.signatureScript || '').toLowerCase();
      if (!FUNDING_SIG_RE.test(sig)) {
        throw new Error(`funding input ${i} did not get a single Schnorr/SIGHASH_ALL signature`);
      }
      if (!deepEqual(without(b, 'signatureScript'), without(a, 'signatureScript'))) {
        throw new Error(`funding input ${i} changed more than its signatureScript`);
      }
    } else if (!deepEqual(b, a)) {
      throw new Error(`wallet mutated non-funding input ${i} (a covenant input must be left intact)`);
    }
  }
  return true;
}

/* Coerce whatever signPskt returns into the signed transaction object. The
 * page needs it as JSON to run the safety rail; a non-JSON return (a raw
 * serialized PSKT) is refused rather than broadcast unverified. */
function coerceSignedTx(returned) {
  if (returned == null) throw new Error('signPskt returned nothing');
  let obj = returned;
  if (typeof returned === 'string') {
    const s = returned.trim();
    if (s[0] !== '{') {
      throw new Error(
        'signPskt returned a non-JSON string; the trade page needs the signed transaction as ' +
          'JSON to verify the wallet touched only the funding inputs',
      );
    }
    obj = JSON.parse(s);
  }
  if (obj && Array.isArray(obj.inputs)) return obj;
  /* some wallet builds wrap the tx */
  for (const key of ['transaction', 'tx', 'signedTx']) {
    if (obj && obj[key] && Array.isArray(obj[key].inputs)) return obj[key];
  }
  throw new Error('signPskt returned an object without an inputs array');
}

/* Ask KasWare to sign ONLY the funding indexes, then prove it did exactly
 * that. `txJsonString` is the page-built transaction; the returned string is
 * the same transaction with the funding inputs signed and everything else
 * untouched — ready for submit(). Throws (and never broadcasts) on any
 * deviation the safety rail catches. */
export async function signFunding(txJsonString, fundingIndexes) {
  if (typeof txJsonString !== 'string') {
    throw new Error('signFunding needs the transaction as a JSON string');
  }
  if (!Array.isArray(fundingIndexes) || fundingIndexes.length === 0) {
    throw new Error('signFunding needs at least one funding input index to sign');
  }
  const before = JSON.parse(txJsonString);
  const ks = requireWallet();
  /* ALWAYS pass signInputs — with no options KasWare signs every input, which
   * would clobber the page-built covenant sigscripts. */
  const signInputs = fundingIndexes.map((index) => ({ index: Number(index), sighashType: SIGHASH_ALL }));
  const returned = await ks.signPskt({ txJsonString, options: { signInputs } });
  const after = coerceSignedTx(returned);
  assertIntentPreserved(before, after, fundingIndexes);
  /* Prefer the wallet's own string when it gave one back (no re-serialisation
   * drift); otherwise serialise the verified object. */
  return typeof returned === 'string' && returned.trim()[0] === '{'
    ? returned
    : JSON.stringify(after);
}

/* ----------------------------- submission -------------------------------- */

/* A node that already holds the tx is a success, not a failure: a rebuilt or
 * retried submit must be idempotent. */
function looksAlreadyAccepted(text) {
  return /already|duplicate|in the mempool|orphan/i.test(String(text || ''));
}

/* Broadcast the signed transaction. api.kaspa.org first (CORS-open, verified),
 * the vendored WASM SDK over wRPC as fallback. Returns { ok, txid, via } and
 * treats an already-in-mempool answer as success. Throws only when NO node
 * accepted it — it never reports a broadcast that did not happen.
 *
 * The transaction must be the kaspad RPC model the page built and the wallet
 * signed (inputs[].previousOutpoint, outputs[].amount, scriptPublicKey with a
 * hex `scriptPublicKey`) — the model api.kaspa.org accepts natively. A caller
 * may inject opts.submitter(tx) or a connected opts.rpc to bypass both. */
/* The wallet takes a transaction as the SDK's canonical "safe JSON" string and
 * parses it with the same library, so the serialization has to be the SDK's
 * own — a hand-rolled JSON that merely looks right could round-trip into a
 * different transaction than the one the reviewer approved. The builder's
 * object is already SDK-shaped: the assembly proof feeds this exact structure
 * to `new Transaction(...)` and recovers both real trades' transaction ids. */
export async function serializeTransaction(transaction) {
  if (!transaction || typeof transaction !== 'object') {
    throw new Error('serializeTransaction needs the built transaction object');
  }
  const kaspa = await loadSdk();
  const tx = new kaspa.Transaction(transaction);
  const json = tx.serializeToSafeJSON();
  if (typeof json !== 'string' || !json.length) {
    throw new Error('the SDK returned no serialized transaction');
  }
  return json;
}

export async function submit(signedTx, opts = {}) {
  const tx = typeof signedTx === 'string' ? JSON.parse(signedTx) : signedTx;
  if (!tx || !Array.isArray(tx.inputs) || !Array.isArray(tx.outputs)) {
    throw new Error('submit needs a transaction with inputs and outputs');
  }
  if (typeof opts.submitter === 'function') {
    return opts.submitter(tx);
  }
  const errors = [];

  try {
    const res = await fetch(`${KASPA_API}/transactions`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', accept: 'application/json' },
      body: JSON.stringify({ transaction: tx, allowOrphan: false }),
    });
    const text = await res.text();
    if (res.ok) {
      let body = {};
      try {
        body = text ? JSON.parse(text) : {};
      } catch (_) {
        /* some deployments answer 200 with the bare txid */
      }
      const txid = body.transactionId || body.txId || body.transaction_id || (text || '').trim() || null;
      return { ok: true, txid, via: 'api.kaspa.org' };
    }
    if (looksAlreadyAccepted(text)) {
      return { ok: true, txid: null, duplicate: true, via: 'api.kaspa.org' };
    }
    errors.push(`api.kaspa.org HTTP ${res.status}: ${text.slice(0, 400)}`);
  } catch (e) {
    errors.push(`api.kaspa.org: ${e && e.message ? e.message : e}`);
  }

  try {
    return await submitViaSdk(tx, opts);
  } catch (e) {
    const msg = e && e.message ? e.message : String(e);
    if (looksAlreadyAccepted(msg)) return { ok: true, txid: null, duplicate: true, via: 'wrpc' };
    errors.push(`wrpc: ${msg}`);
  }

  throw new Error(`submit failed on every path: ${errors.join(' | ')}`);
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

/* Map the kaspad RPC transaction model onto the SDK's ITransaction shape
 * (amount -> value, scriptPublicKey.scriptPublicKey -> .script, integers ->
 * BigInt). Kept explicit and narrow: the SDK path is the fallback, and a
 * conversion that silently dropped a field would be worse than a clear throw.
 *
 * NOTE: this wRPC fallback and its reshape are not runtime-verified in this
 * environment (they need a wallet, a browser and a live node). The trusted,
 * exercised path is api.kaspa.org above. */
export function rpcModelToITransaction(tx) {
  /* The SDK's safe JSON writes a ScriptPublicKey as ONE hex string whose first
     four characters are the u16 version: "0000" + "aa20…87". Taking that whole
     string as the script is what handed the node a script beginning 0000aa20
     and earned "non-standard script form". Split it back apart; an object form
     is passed through unchanged. */
  const spk = (s) => {
    if (s == null) throw new Error('output missing scriptPublicKey');
    if (typeof s === 'string') {
      const hex = s.trim().toLowerCase();
      if (hex.length < 6 || hex.length % 2 !== 0 || /[^0-9a-f]/.test(hex)) {
        throw new Error(`scriptPublicKey is not a hex string: ${s}`);
      }
      return { version: parseInt(hex.slice(0, 4), 16), script: hex.slice(4) };
    }
    const script = s.script ?? s.scriptPublicKey;
    if (script == null) throw new Error('output scriptPublicKey missing its script hex');
    return { version: Number(s.version ?? 0), script: String(script) };
  };
  return {
    version: Number(tx.version ?? 0),
    inputs: tx.inputs.map((i) => {
      /* Safe JSON flattens the outpoint onto the input (transactionId/index at
         the top level); an ITransaction nests it. Resolve once and reuse, so
         the utxo entry can never disagree with the input it belongs to. */
      const op = i.previousOutpoint || {};
      const outpoint = {
        transactionId: String(op.transactionId ?? i.transactionId),
        index: Number(op.index ?? i.index ?? 0),
      };
      return {
        previousOutpoint: outpoint,
        signatureScript: i.signatureScript || '',
        sequence: BigInt(i.sequence ?? 0),
        /* A v1 input commits a compute budget; the legacy sig-op count must
           stay 0 or the node calls it inconsistent with the version. Defaulting
           either of these silently rewrote what the trader approved. */
        sigOpCount: Number(i.sigOpCount ?? 0),
        computeBudget: Number(i.computeBudget ?? 0),
        /* The SDK's Transaction constructor requires a utxo entry per input,
           but it is rebuilt here from the fields we know rather than forwarded:
           a wallet round-trip can hand back an `address` the SDK then refuses
           ("The address is invalid"), and none of it is consensus data — the
           node resolves the real outpoints itself. */
        ...(i.utxo
          ? {
              utxo: {
                outpoint,
                amount: BigInt(i.utxo.amount ?? 0),
                scriptPublicKey: spk(i.utxo.scriptPublicKey),
                blockDaaScore: BigInt(i.utxo.blockDaaScore ?? 0),
                isCoinbase: Boolean(i.utxo.isCoinbase ?? false),
              },
            }
          : {}),
      };
    }),
    outputs: tx.outputs.map((o) => {
      const out = {
        value: BigInt(o.amount ?? o.value),
        scriptPublicKey: spk(o.scriptPublicKey),
      };
      /* The covenant binding is what makes an output that creates a covenant
         cell standard at all. Dropping it here handed the node a bare P2SH and
         earned "non-standard script form" on output 0. */
      if (o.covenant) out.covenant = o.covenant;
      return out;
    }),
    lockTime: BigInt(tx.lockTime ?? 0),
    subnetworkId: String(tx.subnetworkId ?? '0000000000000000000000000000000000000000'),
    gas: BigInt(tx.gas ?? 0),
    payload: String(tx.payload ?? ''),
  };
}

async function submitViaSdk(tx, opts) {
  const kaspa = await loadSdk();
  const transaction = new kaspa.Transaction(rpcModelToITransaction(tx));
  let rpc = opts.rpc || null;
  let owned = false;
  if (!rpc) {
    rpc = new kaspa.RpcClient({
      resolver: new kaspa.Resolver(),
      networkId: opts.network || 'mainnet',
      encoding: kaspa.Encoding.Borsh,
    });
    await rpc.connect();
    owned = true;
  }
  try {
    const res = await rpc.submitTransaction({ transaction, allowOrphan: false });
    return { ok: true, txid: res && (res.transactionId || res.txId) ? res.transactionId || res.txId : null, via: 'wrpc' };
  } finally {
    if (owned) {
      try {
        await rpc.disconnect();
      } catch (_) {
        /* best-effort teardown */
      }
    }
  }
}
