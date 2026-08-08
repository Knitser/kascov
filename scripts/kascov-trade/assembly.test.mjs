// Acceptance gate for the assembly layer in web/kascov/curve.js.
//
// buildBuy/buildSell are handed EXACTLY what two real, hash-matched $KASCOV
// trades were handed — the same curve cell, the same inventory cell, the same
// token cell, the same funding UTXO, the same key, the same amount, the same
// network fee — and the transaction they assemble is compared to the real one
// byte for byte: every input's previous outpoint, every covenant input's whole
// signatureScript (the 172KB curve reveal and the 2.4KB kcc20 transfers), and
// every output's value and scriptPublicKey, in order.
//
// The single range that cannot be reproduced here is the trader's own Schnorr
// signature on the funding input: producing it would mean holding their private
// key. The builder emits that slot EMPTY (the wallet fills it, and only it), so
// the test asserts the slot is empty, then splices the real signature in and
// checks the finished transaction's id against the chain. An id match closes
// over version, sequence, lock time, subnetwork, gas, payload, the output
// covenant bindings and every outpoint, value and script at once.
//
//   node --test scripts/kascov-trade/assembly.test.mjs
//
// Fee/mass note: `feeSompi` is passed explicitly so the change output lands on
// the real trade's value. The fee POLICY (floor, mass headroom, cap) is
// exercised separately below; it is not, and cannot be, proven by a fixture.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

import * as kaspa from "./sdk-node/kaspa.js";

import {
  buildBuy,
  buildSell,
  buildKcc20TransferArgs,
  parseCellState,
  parseCurveState,
  parseScriptPushes,
  spliceCellState,
  p2shScriptPubKey,
  p2pkScriptPubKey,
  CELL_DUST_SOMPI,
  MIN_CHANGE_SOMPI,
  NETWORK_FEE_FLOOR_SOMPI,
  NETWORK_FEE_CAP_SOMPI,
  GRADUATION_KAS_SOMPI,
} from "../../web/kascov/curve.js";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = join(HERE, "..", "..");
const MARKET_COVENANT_ID = "081249d5e7ed184782f86e9647daa7bef936d0feb14b9686b7118b4e70679c1e";
const KASCOV_TOKEN_ID = "c58c826d0aa9cee62f93208718c674883f5c89a8aca4933dc41fb0391539abe2";

const loadFixture = (n) => JSON.parse(readFileSync(join(HERE, n), "utf8"));
function toBytes(hex) {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return out;
}
function toHex(b) {
  let s = "";
  for (const x of b) s += x.toString(16).padStart(2, "0");
  return s;
}
const revealOf = (sigScript) => toHex(parseScriptPushes(sigScript).at(-1).data);
function argBig(p) {
  if (p.data === null) {
    if (p.opcode === 0x00) return 0n;
    if (p.opcode >= 0x51 && p.opcode <= 0x60) return BigInt(p.opcode - 0x50);
    throw new Error("not numeric");
  }
  let v = 0n;
  for (let i = p.data.length - 1; i >= 0; i--) v = (v << 8n) | BigInt(p.data[i]);
  const sign = 1n << BigInt(8 * p.data.length - 1);
  return v & sign ? -(v ^ sign) : v;
}

// Byte-for-byte hex equality that names the first differing byte, with context.
function assertHexEqual(actual, expected, label) {
  const a = String(actual || "").toLowerCase();
  const e = String(expected || "").toLowerCase();
  if (a === e) return;
  const n = Math.min(a.length, e.length) / 2;
  let i = 0;
  for (; i < n; i++) if (a.slice(i * 2, i * 2 + 2) !== e.slice(i * 2, i * 2 + 2)) break;
  const window = (s) => s.slice(Math.max(0, i * 2 - 16), i * 2 + 18);
  assert.fail(
    `${label}: first differing byte at offset ${i} of ${a.length / 2} built / ${e.length / 2} on-chain\n` +
      `   built    …${window(a)}…  (byte 0x${a.slice(i * 2, i * 2 + 2) || "--"})\n` +
      `   on-chain …${window(e)}…  (byte 0x${e.slice(i * 2, i * 2 + 2) || "--"})`,
  );
}

// The scriptPublicKey each input spent, from the chain's own address for that
// outpoint — so the builder's P2SH/P2PK derivation is checked against kaspad's
// encoder, not against itself.
const prevScript = (inp) => kaspa.payToAddressScript(inp.previous_outpoint_address).script;
const outpointOf = (inp) => ({
  transactionId: inp.previous_outpoint_hash,
  index: Number(inp.previous_outpoint_index),
});
const realFee = (tx) =>
  tx.inputs.reduce((s, i) => s + BigInt(i.previous_outpoint_amount), 0n) -
  tx.outputs.reduce((s, o) => s + BigInt(o.amount), 0n);

// Everything the page would have known at build time, lifted from the trade's
// own inputs. Nothing is taken from the trade's outputs.
function sceneFrom(tx) {
  const curveIn = tx.inputs[0];
  const program = revealOf(curveIn.signature_script);
  const parsed = parseCurveState(program);
  return {
    program,
    parsed,
    state: {
      skeletonSupported: true,
      programHex: program,
      marketCovenantId: MARKET_COVENANT_ID,
      reserve: parsed.reserve,
      value: BigInt(curveIn.previous_outpoint_amount),
      liveUtxo: {
        outpoint: outpointOf(curveIn),
        valueSompi: String(curveIn.previous_outpoint_amount),
        scriptPublicKey: { version: 0, script: prevScript(curveIn) },
        count: 1,
      },
    },
    inventoryCell: {
      outpoint: outpointOf(tx.inputs[1]),
      valueSompi: String(tx.inputs[1].previous_outpoint_amount),
      programHex: revealOf(tx.inputs[1].signature_script),
      scriptPublicKey: { version: 0, script: prevScript(tx.inputs[1]) },
    },
  };
}

// The whole comparison, run identically for both trades.
function assertMatchesChain(built, tx, fundingInputIndexes) {
  const t = built.transaction;
  assert.equal(t.inputs.length, tx.inputs.length, "input count");
  assert.equal(t.outputs.length, tx.outputs.length, "output count");

  for (let i = 0; i < t.inputs.length; i++) {
    assert.equal(t.inputs[i].previousOutpoint.transactionId, tx.inputs[i].previous_outpoint_hash, `input ${i} prevout txid`);
    assert.equal(t.inputs[i].previousOutpoint.index, Number(tx.inputs[i].previous_outpoint_index), `input ${i} prevout index`);
    assert.equal(t.inputs[i].utxo.amount, BigInt(tx.inputs[i].previous_outpoint_amount), `input ${i} spent value`);
    assertHexEqual(t.inputs[i].utxo.scriptPublicKey.script, prevScript(tx.inputs[i]), `input ${i} spent script`);
    if (fundingInputIndexes.includes(i)) {
      // The trader's signature — the one thing this builder must NOT produce.
      assert.equal(t.inputs[i].signatureScript, "", `funding input ${i} must go out unsigned`);
      assert.match(tx.inputs[i].signature_script, /^41[0-9a-f]{130}$/, `real funding input ${i} is a lone Schnorr push`);
    } else {
      assertHexEqual(t.inputs[i].signatureScript, tx.inputs[i].signature_script, `input ${i} signatureScript`);
      console.log(`  input ${i} signatureScript matches on-chain (${t.inputs[i].signatureScript.length / 2} bytes)`);
    }
  }
  for (let i = 0; i < t.outputs.length; i++) {
    assertHexEqual(t.outputs[i].scriptPublicKey.script, tx.outputs[i].script_public_key, `output ${i} scriptPublicKey`);
    assert.equal(t.outputs[i].value, BigInt(tx.outputs[i].amount), `output ${i} value`);
    console.log(`  output ${i} ${String(t.outputs[i].value).padStart(15)} -> ${t.outputs[i].scriptPublicKey.script}`);
  }

  // Splice the real signature in and let the SDK judge the whole thing.
  const signed = {
    ...t,
    inputs: t.inputs.map((inp, i) =>
      fundingInputIndexes.includes(i) ? { ...inp, signatureScript: tx.inputs[i].signature_script } : inp,
    ),
  };
  const T = new kaspa.Transaction(signed);
  console.log(`  reconstructed transaction id ${T.id}`);
  assert.equal(T.id, tx.transaction_id, "reconstructed transaction id equals the on-chain txid");
}

test("BUY: buildBuy reassembles a real $KASCOV buy byte-for-byte", () => {
  const tx = loadFixture("fixture-buy.json");
  const scene = sceneFrom(tx);
  const args = parseScriptPushes(tx.inputs[0].signature_script);
  const kasIn = argBig(args[0]);
  const buyerKey = toHex(args[6].data);
  const fee = realFee(tx);
  console.log(`  BUY kasIn ${kasIn} sompi, buyer ${buyerKey}, network fee ${fee} sompi`);

  const built = buildBuy(scene.state, {
    kasInSompi: kasIn,
    buyerXOnlyPubkey: buyerKey,
    inventoryCell: scene.inventoryCell,
    fundingUtxos: [
      {
        outpoint: outpointOf(tx.inputs[2]),
        amountSompi: String(tx.inputs[2].previous_outpoint_amount),
        scriptPublicKey: { version: 0, script: prevScript(tx.inputs[2]) },
      },
    ],
    feeSompi: fee,
    payload: tx.payload || "",
  });

  assert.deepEqual(built.fundingIndexes, [2], "the buy funds from input 2");
  assert.deepEqual(built.signInputs, [{ index: 2, sighashType: 1 }], "only input 2 is offered for signature");
  assertMatchesChain(built, tx, [2]);

  const v = built.verify();
  console.log(`  verify -> ok=${v.ok} over ${v.checks.length} checks`);
  assert.equal(v.ok, true, v.reason || "");
  assert.equal(built.networkFeeSompi, fee);
  assert.equal(built.changeSompi, BigInt(tx.outputs[6].amount));
});

test("SELL: buildSell reassembles a real $KASCOV sell byte-for-byte", () => {
  const tx = loadFixture("fixture-sell.json");
  const scene = sceneFrom(tx);
  const args = parseScriptPushes(tx.inputs[0].signature_script);
  const tokenIn = argBig(args[0]);
  const kasOut = argBig(args[1]);
  const sellerKey = toHex(args[6].data);
  const fee = realFee(tx);
  console.log(`  SELL tokenIn ${tokenIn}, kasOut ${kasOut} sompi, seller ${sellerKey}, network fee ${fee} sompi`);

  const built = buildSell(scene.state, {
    kasOutSompi: kasOut,
    sellerXOnlyPubkey: sellerKey,
    inventoryCell: scene.inventoryCell,
    tokenCells: [
      {
        outpoint: outpointOf(tx.inputs[2]),
        valueSompi: String(tx.inputs[2].previous_outpoint_amount),
        programHex: revealOf(tx.inputs[2].signature_script),
        scriptPublicKey: { version: 0, script: prevScript(tx.inputs[2]) },
      },
    ],
    fundingUtxos: [
      {
        outpoint: outpointOf(tx.inputs[3]),
        amountSompi: String(tx.inputs[3].previous_outpoint_amount),
        scriptPublicKey: { version: 0, script: prevScript(tx.inputs[3]) },
      },
    ],
    feeSompi: fee,
    payload: tx.payload || "",
  });

  assert.deepEqual(built.fundingIndexes, [3], "the sell funds from input 3");
  assert.deepEqual(built.signInputs, [{ index: 3, sighashType: 1 }], "only input 3 is offered for signature");
  assertMatchesChain(built, tx, [3]);

  const v = built.verify();
  console.log(`  verify -> ok=${v.ok} over ${v.checks.length} checks`);
  assert.equal(v.ok, true, v.reason || "");
  assert.equal(built.changeSompi, BigInt(tx.outputs[5].amount));
});

test("the kcc20 transfer args rebuild all three real cell sigscripts", () => {
  const buy = loadFixture("fixture-buy.json");
  const sell = loadFixture("fixture-sell.json");
  const buyerKey = toHex(parseScriptPushes(buy.inputs[0].signature_script)[6].data);

  // BUY: one kcc20 input (the inventory), two new states, witness -> input 0.
  const b = buildKcc20TransferArgs({
    newStates: [
      { owner: MARKET_COVENANT_ID, identifierType: 0x02, amount: 276454n, isMinter: 0 },
      { owner: buyerKey, identifierType: 0x03, amount: 191n, isMinter: 0 },
    ],
    witnesses: [0],
    parentProgramHex: revealOf(buy.inputs[1].signature_script),
  });
  assertHexEqual(b.signatureScript, buy.inputs[1].signature_script, "buy inventory kcc20 args");

  // SELL: two kcc20 inputs sharing ONE new state; the inventory is authorized by
  // the curve input, the seller's cell by the seller's P2PK input at index 3.
  for (const idx of [1, 2]) {
    const s = buildKcc20TransferArgs({
      newStates: [{ owner: MARKET_COVENANT_ID, identifierType: 0x02, amount: 276645n, isMinter: 0 }],
      witnesses: [0, 3],
      parentProgramHex: revealOf(sell.inputs[idx].signature_script),
    });
    assertHexEqual(s.signatureScript, sell.inputs[idx].signature_script, `sell kcc20 args, input ${idx}`);
  }
  console.log("  3/3 kcc20 signatureScripts rebuilt byte-for-byte");
});

test("a cell's whole program is derivable from the shipped template plus its state head", () => {
  const buy = loadFixture("fixture-buy.json");
  const sell = loadFixture("fixture-sell.json");
  const template = toHex(new Uint8Array(readFileSync(join(REPO, "crates/kascov-decode/fixtures/kcc20_unguarded_kron.bin"))));

  // the buy's inventory parent, rebuilt from the template alone…
  const parent = revealOf(buy.inputs[1].signature_script);
  const st = parseCellState(parent);
  const derived = spliceCellState(template, st.owner, st.identifierType, st.amount, st.isMinter);
  assertHexEqual(derived, parent, "derived inventory program");
  // …and it must hash to the script the previous trade committed to.
  assert.equal(p2shScriptPubKey(derived), sell.outputs[1].script_public_key, "derived program hashes to the live cell");

  const seller = parseCellState(revealOf(sell.inputs[2].signature_script));
  assert.equal(seller.identifierType, 0x03, "a holder cell is presence-owned");
  assert.equal(seller.amount, 1376n, "the seller's cell held 1376 $KASCOV");
  console.log(`  inventory ${st.amount} $KASCOV owned by ${st.owner.slice(0, 12)}…, seller cell ${seller.amount}`);
});

test("the curve names $KASCOV and the inventory carries its whole reserve", () => {
  for (const name of ["fixture-buy.json", "fixture-sell.json"]) {
    const tx = loadFixture(name);
    const scene = sceneFrom(tx);
    assert.equal(scene.parsed.tokenCovid, KASCOV_TOKEN_ID);
    assert.equal(parseCellState(scene.inventoryCell.programHex).amount, scene.parsed.reserve);
    assert.equal(BigInt(scene.inventoryCell.valueSompi), CELL_DUST_SOMPI);
  }
});

// ── the refusals ──────────────────────────────────────────────────────────────

function buyScene() {
  const tx = loadFixture("fixture-buy.json");
  const scene = sceneFrom(tx);
  const args = parseScriptPushes(tx.inputs[0].signature_script);
  return {
    tx,
    scene,
    params: {
      kasInSompi: argBig(args[0]),
      buyerXOnlyPubkey: toHex(args[6].data),
      inventoryCell: scene.inventoryCell,
      fundingUtxos: [
        {
          outpoint: outpointOf(tx.inputs[2]),
          amountSompi: String(tx.inputs[2].previous_outpoint_amount),
          scriptPublicKey: { version: 0, script: prevScript(tx.inputs[2]) },
        },
      ],
      feeSompi: realFee(tx),
      payload: tx.payload || "",
    },
  };
}

test("REFUSES: an unsupported market build", () => {
  const { scene, params } = buyScene();
  assert.throws(() => buildBuy({ ...scene.state, skeletonSupported: false }, params), /not the skeleton/);
  assert.throws(() => buildBuy({ ...scene.state, skeletonSupported: undefined }, params), /not the skeleton/);
});

test("REFUSES: a curve showing more than one live cell", () => {
  const { scene, params } = buyScene();
  const state = { ...scene.state, liveUtxo: { ...scene.state.liveUtxo, count: 2 } };
  assert.throws(() => buildBuy(state, params), /2 live cells/);
});

test("REFUSES: a kasIn that is not a whole SCALE step", () => {
  const { scene, params } = buyScene();
  assert.throws(
    () => buildBuy(scene.state, { ...params, kasInSompi: params.kasInSompi + 1n }),
    /not a whole multiple of SCALE/,
  );
});

test("REFUSES: a sell whose cells cannot deliver exactly the requested kasOut", () => {
  const tx = loadFixture("fixture-sell.json");
  const scene = sceneFrom(tx);
  const args = parseScriptPushes(tx.inputs[0].signature_script);
  const base = {
    sellerXOnlyPubkey: toHex(args[6].data),
    inventoryCell: scene.inventoryCell,
    tokenCells: [
      {
        outpoint: outpointOf(tx.inputs[2]),
        valueSompi: String(tx.inputs[2].previous_outpoint_amount),
        programHex: revealOf(tx.inputs[2].signature_script),
        scriptPublicKey: { version: 0, script: prevScript(tx.inputs[2]) },
      },
    ],
    fundingUtxos: [
      {
        outpoint: outpointOf(tx.inputs[3]),
        amountSompi: String(tx.inputs[3].previous_outpoint_amount),
      },
    ],
    feeSompi: realFee(tx),
  };
  const kasOut = argBig(args[1]);
  // one SCALE step more than the cell can deliver
  assert.throws(() => buildSell(scene.state, { ...base, kasOutSompi: kasOut + 1_000_000n }), /partial-cell sells/);
  // and a request that is not even a SCALE multiple
  assert.throws(() => buildSell(scene.state, { ...base, kasOutSompi: kasOut + 1n }), /not a whole multiple of SCALE/);
  // two seller cells is a witness layout no observed trade proves
  assert.throws(
    () => buildSell(scene.state, { ...base, kasOutSompi: kasOut, tokenCells: [base.tokenCells[0], base.tokenCells[0]] }),
    /not proven by any observed trade/,
  );
});

test("REFUSES: a stale inventory cell, a mis-owned cell, and foreign funding", () => {
  const { tx, scene, params } = buyScene();
  const template = toHex(new Uint8Array(readFileSync(join(REPO, "crates/kascov-decode/fixtures/kcc20_unguarded_kron.bin"))));

  // an inventory cell committing to a different balance than the curve does
  const stale = spliceCellState(template, MARKET_COVENANT_ID, 0x02, scene.parsed.reserve - 1n, 0);
  assert.throws(
    () => buildBuy(scene.state, { ...params, inventoryCell: { ...scene.inventoryCell, programHex: stale, scriptPublicKey: null } }),
    /stale inventory cell/,
  );
  // a program that does not hash to the script the cell committed to
  assert.throws(
    () => buildBuy(scene.state, { ...params, inventoryCell: { ...scene.inventoryCell, programHex: stale } }),
    /does not hash to its committed script/,
  );
  // an inventory cell owned by someone other than the market
  const foreign = spliceCellState(template, params.buyerXOnlyPubkey, 0x02, scene.parsed.reserve, 0);
  assert.throws(
    () => buildBuy(scene.state, { ...params, inventoryCell: { ...scene.inventoryCell, programHex: foreign, scriptPublicKey: null } }),
    /is owned by/,
  );
  // funding that is not the trader's own P2PK
  assert.throws(
    () =>
      buildBuy(scene.state, {
        ...params,
        fundingUtxos: [{ ...params.fundingUtxos[0], scriptPublicKey: { version: 0, script: p2pkScriptPubKey("11".repeat(32)) } }],
      }),
    /does not pay the user's P2PK key/,
  );
  assert.equal(tx.inputs.length, 3);
});

test("REFUSES: routing change anywhere but the trader's own key", () => {
  const { scene, params } = buyScene();
  assert.throws(
    () => buildBuy(scene.state, { ...params, changeScriptPublicKey: p2pkScriptPubKey("22".repeat(32)) }),
    /change may only pay the trader's own P2PK key/,
  );
  assert.throws(
    () => buildBuy(scene.state, { ...params, changeAddress: "kaspa:qsomething", addressToScript: () => p2pkScriptPubKey("33".repeat(32)) }),
    /does not decode to the trader's own P2PK key/,
  );
  // the trader's own address is accepted, and still produces the real trade
  const ok = buildBuy(scene.state, {
    ...params,
    changeAddress: "kaspa:qrftmz2j3ps3szdwzh9sfxhxux6ws0aqrx8kkm48ag4r07487azps04dlllun",
    addressToScript: (a) => kaspa.payToAddressScript(a).script,
  });
  assert.equal(ok.verify().ok, true);
});

test("REFUSES: a buy that would carry the curve past its graduation target", () => {
  const { tx, scene, params } = buyScene();
  // the real curve sat at 15.2M KAS against a 25M KAS target; ask for the rest
  const room = GRADUATION_KAS_SOMPI - scene.state.value;
  const big = ((room / 1_000_000n) + 1n) * 1_000_000n;
  const funded = [{ ...params.fundingUtxos[0], amountSompi: String(big * 2n) }];
  assert.throws(
    () => buildBuy(scene.state, { ...params, kasInSompi: big, fundingUtxos: funded }),
    /past the .* graduation target/,
  );
  // one SCALE step under the target still builds
  const ok = buildBuy(scene.state, { ...params, kasInSompi: big - 1_000_000n, fundingUtxos: funded });
  assert.equal(ok.verify().ok, true);
  assert.ok(ok.transaction.outputs[0].value <= GRADUATION_KAS_SOMPI);
  console.log(`  headroom ${room} sompi; ${big} refused, ${big - 1_000_000n} accepted`);
  assert.equal(tx.inputs.length, 3);
});

test("REFUSES: a curve program that does not hash to the live cell", () => {
  const { scene, params } = buyScene();
  const bent = scene.state.programHex.slice(0, 200) + (scene.state.programHex[200] === "0" ? "1" : "0") + scene.state.programHex.slice(201);
  assert.throws(() => buildBuy({ ...scene.state, programHex: bent }, params), /does not hash to the live cell/);
});

test("REFUSES: funding that cannot cover the trade", () => {
  const { scene, params } = buyScene();
  assert.throws(
    () => buildBuy(scene.state, { ...params, fundingUtxos: [{ ...params.fundingUtxos[0], amountSompi: "100000000" }] }),
    /funding is short/,
  );
});

test("REFUSES: a fee above the safety cap, and a change output below the floor", () => {
  const { scene, params } = buyScene();
  assert.throws(() => buildBuy(scene.state, { ...params, feeSompi: NETWORK_FEE_CAP_SOMPI + 1n }), /safety cap/);
  // trim the funding so the change lands just under the floor
  const need = BigInt(params.fundingUtxos[0].amountSompi) - BigInt(loadFixture("fixture-buy.json").outputs[6].amount);
  assert.throws(
    () => buildBuy(scene.state, { ...params, fundingUtxos: [{ ...params.fundingUtxos[0], amountSompi: String(need + MIN_CHANGE_SOMPI - 1n) }] }),
    /below the .* floor/,
  );
});

// ── verify() refuses rather than throws ───────────────────────────────────────

test("verify() returns ok:false (never throws) when an output is tampered with", () => {
  const { tx, scene, params } = buyScene();
  const built = buildBuy(scene.state, params);
  assert.equal(built.verify().ok, true);

  const cases = [
    ["a fee leg redirected", () => { built.transaction.outputs[4].scriptPublicKey = { version: 0, script: p2pkScriptPubKey("44".repeat(32)) }; }],
    ["change redirected", () => { built.transaction.outputs[6].scriptPublicKey = { version: 0, script: p2pkScriptPubKey("55".repeat(32)) }; }],
    ["the buyer's cell shorted", () => { built.transaction.outputs[2].value = 1n; }],
    ["the curve short-changed", () => { built.transaction.outputs[0].value -= 1n; }],
    ["a covenant binding dropped", () => { delete built.transaction.outputs[0].covenant; }],
    ["a covenant input's script emptied", () => { built.transaction.inputs[1].signatureScript = ""; }],
    ["the outputs array emptied", () => { built.transaction.outputs = []; }],
  ];
  for (const [name, tamper] of cases) {
    const fresh = buildBuy(scene.state, params);
    Object.assign(built, { transaction: fresh.transaction });
    tamper.call(null);
    const v = built.verify();
    console.log(`  ${name.padEnd(32)} -> ok=${v.ok} : ${v.reason}`);
    assert.equal(v.ok, false, `${name} should fail verification`);
    assert.ok(v.reason, `${name} should carry a reason`);
    assert.ok(Array.isArray(v.checks) && v.checks.length > 0);
  }
  assert.equal(tx.outputs.length, 7);
});

test("verify() rejects a transaction whose funding input was pre-signed by the builder", () => {
  const { scene, params } = buyScene();
  const built = buildBuy(scene.state, params);
  built.transaction.inputs[2].signatureScript = "41" + "ab".repeat(65);
  const v = built.verify();
  assert.equal(v.ok, false);
  assert.match(v.reason, /signing plan/);
});

// ── the fee policy (no fixture can prove this; it is exercised, not proven) ───

test("fee policy: the floor holds, an injected mass can only raise it, the cap bites", () => {
  const { scene, params } = buyScene();
  const noFee = { ...params };
  delete noFee.feeSompi;

  const floored = buildBuy(scene.state, noFee);
  assert.equal(floored.networkFeeSompi, NETWORK_FEE_FLOOR_SOMPI, "with no mass function the floor is the fee");
  assert.equal(floored.feeSource, "floor");
  assert.equal(floored.verify().ok, true);

  // a real mass from the vendored SDK, on the transaction as constructed
  const withMass = buildBuy(scene.state, {
    ...noFee,
    calculateTransactionMass: (net, itx) => kaspa.calculateTransactionMass(net, new kaspa.Transaction(itx)),
  });
  console.log(`  SDK mass ${withMass.massSompi} -> fee ${withMass.networkFeeSompi} (${withMass.feeSource})`);
  assert.ok(withMass.networkFeeSompi >= NETWORK_FEE_FLOOR_SOMPI, "mass may raise the fee, never lower it");
  assert.equal(withMass.verify().ok, true);

  // a mass function that demands more than the cap must refuse, not overpay
  assert.throws(
    () => buildBuy(scene.state, { ...noFee, calculateTransactionMass: () => 10_000_000_000n }),
    /safety cap/,
  );
});

test("largest-first funding selection covers the trade from several UTXOs", () => {
  const { tx, scene, params } = buyScene();
  const total = BigInt(tx.inputs[2].previous_outpoint_amount);
  const spk = { version: 0, script: prevScript(tx.inputs[2]) };
  // three quarters cover the trade; the fourth quarter and the dust stay home
  const split = [
    { outpoint: { transactionId: "aa".repeat(32), index: 0 }, amountSompi: String(total / 4n), scriptPublicKey: spk },
    { outpoint: { transactionId: "bb".repeat(32), index: 1 }, amountSompi: String(total / 4n), scriptPublicKey: spk },
    { outpoint: { transactionId: "cc".repeat(32), index: 2 }, amountSompi: String(total / 4n), scriptPublicKey: spk },
    { outpoint: { transactionId: "dd".repeat(32), index: 3 }, amountSompi: String(total / 4n), scriptPublicKey: spk },
    { outpoint: { transactionId: "ee".repeat(32), index: 4 }, amountSompi: "1000", scriptPublicKey: spk },
  ];
  const built = buildBuy(scene.state, { ...params, fundingUtxos: split });
  assert.deepEqual(built.fundingIndexes, [2, 3, 4], "three inputs taken, the smallest UTXO left behind");
  assert.equal(built.transaction.inputs.length, 5, "curve + inventory + three funding");
  for (const inp of built.transaction.inputs.slice(2)) {
    assert.notEqual(inp.previousOutpoint.transactionId, "ee".repeat(32), "the 1000-sompi dust is never selected");
  }
  assert.equal(built.verify().ok, true);
  // the value equation still closes on the caller's fee, with the surplus in change
  const inSum = built.transaction.inputs.reduce((s, i) => s + i.utxo.amount, 0n);
  const outSum = built.transaction.outputs.reduce((s, o) => s + o.value, 0n);
  assert.equal(inSum - outSum, params.feeSompi);
  assert.equal(built.transaction.outputs.at(-1).value, built.changeSompi);
});

test("the review rows account for every sompi that leaves the trader", () => {
  const { tx, scene, params } = buyScene();
  const built = buildBuy(scene.state, params);
  const rows = built.outputs;
  assert.equal(rows.length, tx.outputs.length + 1, "one row per output plus the network fee");
  for (const r of rows) {
    assert.ok(r.label && r.kind && r.value_sompi != null, `row ${JSON.stringify(r)} is complete`);
    assert.doesNotThrow(() => BigInt(r.value_sompi));
  }
  console.log(rows.map((r) => `  ${r.kind.padEnd(9)} ${String(r.value_sompi).padStart(15)}  ${r.label}`).join("\n"));
  const outputRows = rows.filter((r) => r.kind !== "network");
  assert.equal(
    outputRows.reduce((s, r) => s + BigInt(r.value_sompi), 0n),
    built.transaction.outputs.reduce((s, o) => s + o.value, 0n),
    "the rows total the transaction's outputs",
  );
});
