// Acceptance gate for web/kascov/curve.js.
//
// For each of two real, hash-matched $KASCOV trades this test lifts the parent
// curve program and reserve out of the trade's OWN input-0 reveal, drives the
// builder with the trade's actual amounts, and asserts that every output script
// and value it produces equals the on-chain output byte-for-byte — including the
// continuation program's BLAKE2b P2SH and both token-cell P2SH commitments. If a
// single byte drifts, the builder is wrong; there is no partial credit on a page
// that spends real KAS.
//
//   node --test scripts/kascov-trade/curve.test.mjs

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

import {
  parseCurveState,
  nextCurveProgram,
  p2shScriptPubKey,
  p2pkScriptPubKey,
  quoteBuy,
  quoteSell,
  buildBuyArgs,
  buildSellArgs,
  assembleBuyOutputs,
  assembleSellOutputs,
  feeKeysFromCurveProgram,
  blake2b256,
  CELL_DUST_SOMPI,
} from "../../web/kascov/curve.js";

const HERE = dirname(fileURLToPath(import.meta.url));
const KASCOV_TOKEN_ID = "c58c826d0aa9cee62f93208718c674883f5c89a8aca4933dc41fb0391539abe2";
const MARKET_COVENANT_ID = "081249d5e7ed184782f86e9647daa7bef936d0feb14b9686b7118b4e70679c1e";
// Fee-key slots in the KRON curve v1 program (byte offsets into the 172161-byte
// reveal). The page can therefore source every fee recipient from chain bytes.



function loadFixture(name) {
  return JSON.parse(readFileSync(join(HERE, name), "utf8"));
}
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
// Enumerate the pushes/opcodes of a signatureScript.
function parsePushes(hex) {
  const b = toBytes(hex);
  const out = [];
  let i = 0;
  while (i < b.length) {
    const op = b[i];
    let ds, len;
    if (op >= 0x01 && op <= 0x4b) { ds = i + 1; len = op; }
    else if (op === 0x4c) { len = b[i + 1]; ds = i + 2; }
    else if (op === 0x4d) { len = b[i + 1] | (b[i + 2] << 8); ds = i + 3; }
    else if (op === 0x4e) { len = b[i + 1] | (b[i + 2] << 8) | (b[i + 3] << 16) | (b[i + 4] * 16777216); ds = i + 5; }
    else { out.push({ opcode: op, data: null }); i++; continue; }
    out.push({ opcode: null, data: b.slice(ds, ds + len) });
    i = ds + len;
  }
  return out;
}
// Read a scriptNum-encoded push (or OP_0 / OP_1..16) as a BigInt.
function argToBigInt(p) {
  if (p.data === null) {
    if (p.opcode === 0x00) return 0n;
    if (p.opcode >= 0x51 && p.opcode <= 0x60) return BigInt(p.opcode - 0x50);
    throw new Error("not a numeric arg: opcode " + p.opcode);
  }
  const d = p.data;
  if (d.length === 0) return 0n;
  let v = 0n;
  for (let i = d.length - 1; i >= 0; i--) v = (v << 8n) | BigInt(d[i]);
  // top-bit sign (kasIn/kasOut are always positive here)
  const signBit = 1n << BigInt(8 * d.length - 1);
  if (v & signBit) v = -(v ^ signBit);
  return v;
}
function outSpk(o) { return o.script_public_key; }

// The reveal (last, largest push) of a signatureScript.
function reveal(sigScript) {
  const ps = parsePushes(sigScript);
  return toHex(ps[ps.length - 1].data);
}
// Find the KCC20 cell base program spent by any input (a ~2.4KB reveal that is
// not the curve program).
function cellBaseFrom(tx) {
  for (const inp of tx.inputs) {
    const ps = parsePushes(inp.signature_script);
    const last = ps[ps.length - 1];
    if (last.data && last.data.length > 1000 && last.data.length < 4000) return toHex(last.data);
  }
  throw new Error("no cell base program among inputs");
}
// The page sources fee recipients from the program itself; prove that helper.
const feeKeysFromProgram = feeKeysFromCurveProgram;

test("BUY: builder reproduces a real $KASCOV buy byte-for-byte", () => {
  const tx = loadFixture("fixture-buy.json");
  const args = parsePushes(tx.inputs[0].signature_script);
  const program = reveal(tx.inputs[0].signature_script);

  // Parent state, lifted from the trade's own reveal + the spent curve value.
  const parsed = parseCurveState(program);
  assert.equal(parsed.tokenCovid, KASCOV_TOKEN_ID, "curve names $KASCOV");
  const state = { reserve: parsed.reserve, value: BigInt(tx.inputs[0].previous_outpoint_amount) };

  // The trade's own inputs: kasIn (arg 0), the buyer key (arg 6), the reserve
  // the covenant committed to after (arg 4), the tokens bought (arg 8).
  const kasIn = argToBigInt(args[0]);
  const onchainTokenOut = argToBigInt(args[8]);
  const onchainNewReserve = argToBigInt(args[4]);
  const buyerKey = toHex(args[6].data);

  // The quote, derived purely from (state, kasIn), must match the chain.
  const q = quoteBuy(state, kasIn);
  console.log("  BUY quote  tokenOut", q.tokenOut, "onchain", onchainTokenOut);
  assert.equal(q.tokenOut, onchainTokenOut, "tokenOut");
  assert.equal(q.newReserve, onchainNewReserve, "reserve after");
  assert.equal(q.curveValueAfter, BigInt(tx.outputs[0].amount), "curve value after");
  assert.equal(q.fees.creator, BigInt(tx.outputs[3].amount), "creator fee");
  assert.equal(q.fees.platform, BigInt(tx.outputs[4].amount), "platform fee");
  assert.equal(q.fees.dev, BigInt(tx.outputs[5].amount), "dev fee");

  // Continuation program P2SH must equal out0's committed script.
  const cont = nextCurveProgram(program, q.newReserve);
  const contSpk = p2shScriptPubKey(cont);
  console.log("  BUY out0 P2SH", contSpk);
  console.log("      on-chain ", outSpk(tx.outputs[0]));
  assert.equal(contSpk, outSpk(tx.outputs[0]), "continuation P2SH == out0");

  // Fee keys sourced from chain bytes alone must equal the on-chain fee legs.
  const feeKeys = feeKeysFromProgram(program);
  assert.equal(p2pkScriptPubKey(feeKeys.creator), outSpk(tx.outputs[3]), "creator key from program");
  assert.equal(p2pkScriptPubKey(feeKeys.platform), outSpk(tx.outputs[4]), "platform key from program");
  assert.equal(p2pkScriptPubKey(feeKeys.dev), outSpk(tx.outputs[5]), "dev key from program");

  // The full output list, byte-for-byte. The change value is free money back to
  // the buyer, so it is taken from the chain; its script is still asserted.
  const outs = assembleBuyOutputs({
    parentProgramHex: program,
    cellBaseProgramHex: cellBaseFrom(tx),
    marketCovenantId: MARKET_COVENANT_ID,
    quote: q,
    buyerXOnlyKey: buyerKey,
    creatorKey: feeKeys.creator,
    platformKey: feeKeys.platform,
    devKey: feeKeys.dev,
    changeSompi: BigInt(tx.outputs[6].amount),
  });
  assert.equal(outs.length, tx.outputs.length, "output count");
  for (let i = 0; i < outs.length; i++) {
    console.log(`  BUY out${i} value ${outs[i].value} spk ${outs[i].scriptPublicKey}`);
    assert.equal(outs[i].scriptPublicKey, outSpk(tx.outputs[i]), `out${i} scriptPublicKey`);
    assert.equal(outs[i].value, BigInt(tx.outputs[i].amount), `out${i} value`);
  }
  // Both dust cells carry the 0.5 KAS floor the covenant fixes.
  assert.equal(outs[1].value, CELL_DUST_SOMPI);
  assert.equal(outs[2].value, CELL_DUST_SOMPI);

  // Bonus: the curve leg's signatureScript rebuilds byte-for-byte.
  const built = buildBuyArgs({
    parentProgramHex: program,
    marketCovenantId: MARKET_COVENANT_ID,
    kasInSompi: kasIn,
    tokenOut: q.tokenOut,
    newReserve: q.newReserve,
    buyerXOnlyKey: buyerKey,
  });
  assert.equal(built.signatureScript, tx.inputs[0].signature_script, "input-0 signatureScript");
});

test("SELL: builder reproduces a real $KASCOV sell byte-for-byte", () => {
  const tx = loadFixture("fixture-sell.json");
  const args = parsePushes(tx.inputs[0].signature_script);
  const program = reveal(tx.inputs[0].signature_script);

  const parsed = parseCurveState(program);
  assert.equal(parsed.tokenCovid, KASCOV_TOKEN_ID, "curve names $KASCOV");
  const state = { reserve: parsed.reserve, value: BigInt(tx.inputs[0].previous_outpoint_amount) };

  const tokenIn = argToBigInt(args[0]);
  const onchainKasOut = argToBigInt(args[1]);
  const onchainNewReserve = argToBigInt(args[4]);
  const sellerKey = toHex(args[6].data);
  const changeAmount = argToBigInt(args[8]);

  const q = quoteSell(state, tokenIn);
  console.log("  SELL quote  kasOut", q.kasOut, "onchain", onchainKasOut);
  assert.equal(q.kasOut, onchainKasOut, "kasOut");
  assert.equal(q.newReserve, onchainNewReserve, "reserve after");
  assert.equal(q.curveValueAfter, BigInt(tx.outputs[0].amount), "curve value after");
  assert.equal(q.fees.creator, BigInt(tx.outputs[2].amount), "creator fee");
  assert.equal(q.fees.platform, BigInt(tx.outputs[3].amount), "platform fee");
  assert.equal(q.fees.dev, BigInt(tx.outputs[4].amount), "dev fee");

  const cont = nextCurveProgram(program, q.newReserve);
  const contSpk = p2shScriptPubKey(cont);
  console.log("  SELL out0 P2SH", contSpk);
  console.log("       on-chain ", outSpk(tx.outputs[0]));
  assert.equal(contSpk, outSpk(tx.outputs[0]), "continuation P2SH == out0");

  const feeKeys = feeKeysFromProgram(program);
  assert.equal(p2pkScriptPubKey(feeKeys.creator), outSpk(tx.outputs[2]), "creator key from program");
  assert.equal(p2pkScriptPubKey(feeKeys.platform), outSpk(tx.outputs[3]), "platform key from program");
  assert.equal(p2pkScriptPubKey(feeKeys.dev), outSpk(tx.outputs[4]), "dev key from program");

  const outs = assembleSellOutputs({
    parentProgramHex: program,
    cellBaseProgramHex: cellBaseFrom(tx),
    marketCovenantId: MARKET_COVENANT_ID,
    quote: q,
    sellerXOnlyKey: sellerKey,
    creatorKey: feeKeys.creator,
    platformKey: feeKeys.platform,
    devKey: feeKeys.dev,
    refundSompi: BigInt(tx.outputs[5].amount),
  });
  assert.equal(outs.length, tx.outputs.length, "output count");
  for (let i = 0; i < outs.length; i++) {
    console.log(`  SELL out${i} value ${outs[i].value} spk ${outs[i].scriptPublicKey}`);
    assert.equal(outs[i].scriptPublicKey, outSpk(tx.outputs[i]), `out${i} scriptPublicKey`);
    assert.equal(outs[i].value, BigInt(tx.outputs[i].amount), `out${i} value`);
  }
  assert.equal(outs[1].value, CELL_DUST_SOMPI);

  const built = buildSellArgs({
    parentProgramHex: program,
    marketCovenantId: MARKET_COVENANT_ID,
    tokenIn,
    kasOutSompi: q.kasOut,
    newReserve: q.newReserve,
    sellerXOnlyKey: sellerKey,
    changeAmount,
  });
  assert.equal(built.signatureScript, tx.inputs[0].signature_script, "input-0 signatureScript");
});

test("cross-fixture: the buy spends the sell's own curve continuation", () => {
  const sell = loadFixture("fixture-sell.json");
  const buy = loadFixture("fixture-buy.json");
  // The sell's out0 is the very UTXO the buy consumes at input 0.
  assert.equal(buy.inputs[0].previous_outpoint_hash, sell.transaction_id);
  assert.equal(Number(buy.inputs[0].previous_outpoint_index), 0);
  // And the buy's parent program hashes to the sell's out0 script — proving the
  // BLAKE2b-256 variant end to end across two independent trades.
  const buyParent = reveal(buy.inputs[0].signature_script);
  assert.equal(p2shScriptPubKey(buyParent), sell.outputs[0].script_public_key);
});

test("BLAKE2b-256 matches the RFC 7693 'abc' vector", () => {
  assert.equal(
    toHex(blake2b256(new TextEncoder().encode("abc"))),
    "bddd813c634239723171ef3fee98579b94964e3bb1cb3e427262c8c068d52319",
  );
});
