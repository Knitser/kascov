/* SPDX-License-Identifier: AGPL-3.0-only
 * Copyright (c) 2026 Michiel Hamblok
 *
 * Part of kascov's direct-trade module. Unlike the rest of this repository
 * (MIT), this module is licensed under the GNU Affero General Public License
 * v3.0: run it, modify it, serve it — and publish your changes the same way.
 * Full text in LICENSE-AGPL at the repository root. The vendored Kaspa WASM
 * SDK under web/kascov/sdk/ keeps its own upstream license and is NOT covered
 * by this notice.
 */
/* Dry-run the trade builder against the LIVE mainnet curve.
 *
 * Reads real state from kascov.io, assembles a real buy with a synthetic
 * funding UTXO, and runs the builder's own verify(). Signs nothing and
 * broadcasts nothing — the funding key is fake, so the transaction could never
 * be completed even by accident. This is the last check before a wallet.
 *
 *   node scripts/kascov-trade/live-build.mjs [kasIn]
 */
/* The sandbox running this has no outbound network, so live responses are
 * snapshotted with curl and replayed through the SAME state.js code path the
 * browser uses: pass a directory of {market,curve-cell,cells}.json as
 * KASCOV_LIVE_DIR. Without it, this fetches kascov.io directly. */
import { readFileSync } from 'node:fs';

const SNAP = process.env.KASCOV_LIVE_DIR;
if (SNAP) {
  const file = (url) => {
    if (url.includes('/curve-cell')) return 'curve-cell.json';
    if (url.includes('/cells')) return 'cells.json';
    if (url.includes('/market')) return 'market.json';
    throw new Error(`no snapshot for ${url}`);
  };
  globalThis.fetch = async (url) => {
    const body = readFileSync(`${SNAP}/${file(String(url))}`, 'utf8');
    return { ok: true, status: 200, json: async () => JSON.parse(body) };
  };
}

const { getCurveState, getInventoryCell } = await import('../../web/kascov/state.js');
const { buildBuy } = await import('../../web/kascov/curve.js');

const DATA = 'https://kascov.io/data';
const KAS = 10n ** 8n;
const kasIn = BigInt(process.argv[2] || 10) * KAS;

/* a key that is not anyone's: the dry run must be unspendable by construction */
const FAKE_BUYER = 'd2bd895288611809ae15cb049ae6e1b4e83fa0198f6b6ea7ea2a37faa7f74418';
const fundingScript = `20${FAKE_BUYER}ac`;

const t0 = Date.now();
const state = await getCurveState({ dataBase: DATA });
const inventory = await getInventoryCell(state.marketCovenantId, { dataBase: DATA });
console.log(`live state read in ${Date.now() - t0}ms`);
console.log(`  curve  reserve ${state.reserve} tokens, value ${state.value} sompi`);
console.log(`  inventory ${inventory.amount} @ ${inventory.outpointText}`);
console.log(`  reserve == inventory: ${String(state.reserve) === String(inventory.amount)}`);
if (state.warnings?.length) console.log('  warnings:', state.warnings);

const funding = [
  {
    outpoint: { transactionId: 'ab'.repeat(32), index: 0 },
    valueSompi: (kasIn + 500n * KAS).toString(),
    scriptPublicKey: fundingScript,
  },
];

const t1 = Date.now();
const built = buildBuy(state, {
  kasInSompi: kasIn,
  buyerXOnlyPubkey: FAKE_BUYER,
  fundingUtxos: funding,
  inventoryCell: inventory,
  /* no change script passed: the builder derives it from the buyer's own
     public key, which is exactly what the page does */
});
console.log(`\nassembled in ${Date.now() - t1}ms`);
console.log(`  inputs ${built.transaction.inputs.length}, outputs ${built.transaction.outputs.length}`);
console.log(`  wallet signs only ${JSON.stringify(built.signInputs)}`);
console.log(`  network fee ${built.networkFeeSompi} (${built.feeSource})`);
for (const o of built.outputs) console.log(`   out ${o.label ?? o.kind}: ${o.value_sompi}`);

const t2 = Date.now();
const v = built.verify();
console.log(`\nverify in ${Date.now() - t2}ms -> ok=${v.ok} over ${v.checks?.length ?? 0} checks`);
if (!v.ok) {
  console.log('  REASON:', v.reason);
  for (const c of v.checks ?? []) if (!c.ok) console.log('   failed:', c.name, c.detail ?? '');
  process.exit(1);
}
/* the covenant input must carry no signature: the script authorises it */
const covSig = built.transaction.inputs[0].signatureScript;
console.log(`\ncovenant input sigscript ${covSig.length / 2} bytes, funding input empty: ` +
  `${built.transaction.inputs[built.signInputs[0].index].signatureScript === ''}`);
console.log('\nDRY RUN OK — nothing signed, nothing broadcast.');
