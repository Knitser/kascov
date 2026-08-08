// KRON curve v1 trade builder — pure, offline, permissionless.
//
// The curve covenant (skeleton "KRON curve v1", covenant id 081249d5e7ed1847…)
// enforces every output of a trade by consensus: any transaction whose curve
// input carries the right ordered pushes and whose outputs satisfy the script
// is a valid trade, with no signature on the curve leg and no dependence on any
// off-chain SDK. This module builds those bytes and quotes the economics. It
// touches no wallet and no network — the page wires funding, signing and submit
// around it.
//
// Ground truth: every formula and byte layout here reproduces two real on-chain
// $KASCOV trades byte-for-byte (see scripts/kascov-trade/curve.test.mjs), and
// the constant-product invariant mirrors crates/kascov-core/src/market.rs, which
// replays all 978 curve trades against it. Do not "improve" a formula without
// re-running that gate: a wrong quantum or rounding direction silently mispays.

// ── Baked program constants (KRON curve v1) ───────────────────────────────────
// KAS moves in whole multiples of SCALE; a smaller step is not a valid trade.
export const SCALE = 1_000_000n; // 0.01 KAS in sompi
// V, the curve's virtual KAS reserve, in the program's own units and in sompi.
// V_SOMPI = V_KAS_UNITS * KAS_QUANTUM_SOMPI. The pair (V + kasReserve) * tokens
// is the conserved product.
export const V_KAS_UNITS = 6_250_000n;
export const KAS_QUANTUM_SOMPI = 1_000_000n;
export const V_SOMPI = V_KAS_UNITS * KAS_QUANTUM_SOMPI; // 6_250_000_000_000
// The curve graduates (stops bonding) once its KAS reserve reaches this.
export const GRADUATION_KAS_SOMPI = 25_000_000_000_000n;
// Each covenant cell carries this much KAS as its dust floor.
export const CELL_DUST_SOMPI = 50_000_000n; // 0.5 KAS
// Fee legs, in basis points of the KAS that moves (kasIn on a buy, kasOut on a
// sell). Dev has a hard floor. Totals 125 bps = 1.25%.
export const FEE_CREATOR_BPS = 25n;
export const FEE_PLATFORM_BPS = 90n;
export const FEE_DEV_BPS = 10n;
export const FEE_DEV_FLOOR_SOMPI = 20_000_000n; // 0.2 KAS
const BPS = 10_000n;
// identifier_type of a covenant cell: 0x02 for the market's inventory, 0x03 for
// a holder balance.
const CELL_TYPE_INVENTORY = 0x02;
const CELL_TYPE_HOLDER = 0x03;
// selector push that steers the covenant down its buy vs sell branch.
const SELECTOR_BUY = 1n;
const SELECTOR_SELL = 2n;

// ── hex helpers ───────────────────────────────────────────────────────────────
function toBytes(hex) {
  if (hex instanceof Uint8Array) return hex;
  if (typeof hex !== "string" || hex.length % 2 !== 0 || /[^0-9a-fA-F]/.test(hex)) {
    throw new Error("curve: expected an even-length hex string");
  }
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return out;
}
function toHex(bytes) {
  let s = "";
  for (let i = 0; i < bytes.length; i++) s += bytes[i].toString(16).padStart(2, "0");
  return s;
}
function concat(arrs) {
  let n = 0;
  for (const a of arrs) n += a.length;
  const out = new Uint8Array(n);
  let o = 0;
  for (const a of arrs) { out.set(a, o); o += a.length; }
  return out;
}
function readU64LE(bytes, off) {
  let v = 0n;
  for (let i = 7; i >= 0; i--) v = (v << 8n) | BigInt(bytes[off + i]);
  return v;
}
function writeU64LE(bytes, off, value) {
  let v = BigInt(value);
  if (v < 0n || v > 0xffffffffffffffffn) throw new Error("curve: u64 out of range");
  for (let i = 0; i < 8; i++) { bytes[off + i] = Number(v & 0xffn); v >>= 8n; }
}

// ── BLAKE2b-256, the exact hash a Kaspa P2SH commitment uses ──────────────────
// Plain BLAKE2b with a 32-byte digest length — no key, no personalization. The
// digest length is part of the parameter block, so a truncated BLAKE2b-512 is a
// DIFFERENT hash and must not be substituted. This variant is proven by the test
// reconstructing four real on-chain P2SH scripts. Ported from the public-domain
// blakejs implementation.
const B2B_IV32 = new Uint32Array([
  0xf3bcc908, 0x6a09e667, 0x84caa73b, 0xbb67ae85, 0xfe94f82b, 0x3c6ef372, 0x5f1d36f1, 0xa54ff53a,
  0xade682d1, 0x510e527f, 0x2b3e6c1f, 0x9b05688c, 0xfb41bd6b, 0x1f83d9ab, 0x137e2179, 0x5be0cd19,
]);
const B2B_SIGMA = new Uint8Array([
  0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3,
  11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4, 7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8,
  9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13, 2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9,
  12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11, 13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10,
  6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5, 10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0,
  0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3,
].map((x) => x * 2));
const B2B_V = new Uint32Array(32);
const B2B_M = new Uint32Array(32);
function b2bAddAA(v, a, b) {
  const o0 = v[a] + v[b];
  let o1 = v[a + 1] + v[b + 1];
  if (o0 >= 0x100000000) o1++;
  v[a] = o0; v[a + 1] = o1;
}
function b2bAddAC(v, a, b0, b1) {
  let o0 = v[a] + b0;
  if (b0 < 0) o0 += 0x100000000;
  let o1 = v[a + 1] + b1;
  if (o0 >= 0x100000000) o1++;
  v[a] = o0; v[a + 1] = o1;
}
function b2bGet32(arr, i) {
  return (arr[i] ^ (arr[i + 1] << 8) ^ (arr[i + 2] << 16) ^ (arr[i + 3] << 24)) >>> 0;
}
function b2bG(a, b, c, d, ix, iy) {
  const x0 = B2B_M[ix], x1 = B2B_M[ix + 1], y0 = B2B_M[iy], y1 = B2B_M[iy + 1];
  b2bAddAA(B2B_V, a, b); b2bAddAC(B2B_V, a, x0, x1);
  let xor0 = B2B_V[d] ^ B2B_V[a], xor1 = B2B_V[d + 1] ^ B2B_V[a + 1];
  B2B_V[d] = xor1; B2B_V[d + 1] = xor0;
  b2bAddAA(B2B_V, c, d);
  xor0 = B2B_V[b] ^ B2B_V[c]; xor1 = B2B_V[b + 1] ^ B2B_V[c + 1];
  B2B_V[b] = (xor0 >>> 24) ^ (xor1 << 8); B2B_V[b + 1] = (xor1 >>> 24) ^ (xor0 << 8);
  b2bAddAA(B2B_V, a, b); b2bAddAC(B2B_V, a, y0, y1);
  xor0 = B2B_V[d] ^ B2B_V[a]; xor1 = B2B_V[d + 1] ^ B2B_V[a + 1];
  B2B_V[d] = (xor0 >>> 16) ^ (xor1 << 16); B2B_V[d + 1] = (xor1 >>> 16) ^ (xor0 << 16);
  b2bAddAA(B2B_V, c, d);
  xor0 = B2B_V[b] ^ B2B_V[c]; xor1 = B2B_V[b + 1] ^ B2B_V[c + 1];
  B2B_V[b] = (xor1 >>> 31) ^ (xor0 << 1); B2B_V[b + 1] = (xor0 >>> 31) ^ (xor1 << 1);
}
function b2bCompress(ctx, last) {
  for (let i = 0; i < 16; i++) { B2B_V[i] = ctx.h[i]; B2B_V[i + 16] = B2B_IV32[i]; }
  B2B_V[24] = B2B_V[24] ^ (ctx.t & 0xffffffff);
  B2B_V[25] = B2B_V[25] ^ Math.floor(ctx.t / 0x100000000);
  if (last) { B2B_V[28] = ~B2B_V[28]; B2B_V[29] = ~B2B_V[29]; }
  for (let i = 0; i < 32; i++) B2B_M[i] = b2bGet32(ctx.b, 4 * i);
  for (let i = 0; i < 12; i++) {
    const s = i * 16;
    b2bG(0, 8, 16, 24, B2B_SIGMA[s + 0], B2B_SIGMA[s + 1]);
    b2bG(2, 10, 18, 26, B2B_SIGMA[s + 2], B2B_SIGMA[s + 3]);
    b2bG(4, 12, 20, 28, B2B_SIGMA[s + 4], B2B_SIGMA[s + 5]);
    b2bG(6, 14, 22, 30, B2B_SIGMA[s + 6], B2B_SIGMA[s + 7]);
    b2bG(0, 10, 20, 30, B2B_SIGMA[s + 8], B2B_SIGMA[s + 9]);
    b2bG(2, 12, 22, 24, B2B_SIGMA[s + 10], B2B_SIGMA[s + 11]);
    b2bG(4, 14, 16, 26, B2B_SIGMA[s + 12], B2B_SIGMA[s + 13]);
    b2bG(6, 8, 18, 28, B2B_SIGMA[s + 14], B2B_SIGMA[s + 15]);
  }
  for (let i = 0; i < 16; i++) ctx.h[i] = ctx.h[i] ^ B2B_V[i] ^ B2B_V[i + 16];
}
export function blake2b256(input) {
  const bytes = input instanceof Uint8Array ? input : toBytes(input);
  const ctx = { b: new Uint8Array(128), h: new Uint32Array(16), t: 0, c: 0 };
  for (let i = 0; i < 16; i++) ctx.h[i] = B2B_IV32[i];
  ctx.h[0] ^= 0x01010000 ^ 32;
  for (let i = 0; i < bytes.length; i++) {
    if (ctx.c === 128) { ctx.t += ctx.c; b2bCompress(ctx, false); ctx.c = 0; }
    ctx.b[ctx.c++] = bytes[i];
  }
  ctx.t += ctx.c;
  while (ctx.c < 128) ctx.b[ctx.c++] = 0;
  b2bCompress(ctx, true);
  const out = new Uint8Array(32);
  for (let i = 0; i < 32; i++) out[i] = (ctx.h[i >> 2] >> (8 * (i & 3))) & 0xff;
  return out;
}

// ── script encoding ───────────────────────────────────────────────────────────
// CScriptNum minimal little-endian encoding (the byte payload, no opcode).
function scriptNumBytes(n) {
  n = BigInt(n);
  if (n === 0n) return new Uint8Array(0);
  const neg = n < 0n;
  let a = neg ? -n : n;
  const bytes = [];
  while (a > 0n) { bytes.push(Number(a & 0xffn)); a >>= 8n; }
  if (bytes[bytes.length - 1] & 0x80) bytes.push(neg ? 0x80 : 0x00);
  else if (neg) bytes[bytes.length - 1] |= 0x80;
  return Uint8Array.from(bytes);
}
// Push a number the way the covenant expects it: OP_0 for zero, OP_1..OP_16 for
// 1..16, otherwise a minimal-length data push of its scriptNum bytes.
function pushNumber(n) {
  n = BigInt(n);
  if (n === 0n) return Uint8Array.from([0x00]);
  if (n >= 1n && n <= 16n) return Uint8Array.from([0x50 + Number(n)]);
  const d = scriptNumBytes(n);
  return pushData(d);
}
// Push raw bytes with the minimal length prefix (direct, PUSHDATA1/2/4).
function pushData(buf) {
  const b = buf instanceof Uint8Array ? buf : toBytes(buf);
  const L = b.length;
  if (L < 0x4c) return concat([Uint8Array.from([L]), b]);
  if (L <= 0xff) return concat([Uint8Array.from([0x4c, L]), b]);
  if (L <= 0xffff) {
    const p = new Uint8Array(3); p[0] = 0x4d; p[1] = L & 0xff; p[2] = (L >> 8) & 0xff;
    return concat([p, b]);
  }
  const p = new Uint8Array(5);
  p[0] = 0x4e; p[1] = L & 0xff; p[2] = (L >> 8) & 0xff; p[3] = (L >> 16) & 0xff; p[4] = (L >>> 24) & 0xff;
  return concat([p, b]);
}
// Push a BYTE ARRAY under the consensus minimal-push rule: the empty array is
// OP_0, a lone byte 1..16 is OP_1..OP_16, a lone 0x81 is OP_1NEGATE, everything
// else is a length-prefixed data push. This is NOT pushNumber: a one-byte array
// holding 0x00 is `0100` (a real 1-byte push), because OP_0 would push the EMPTY
// array instead. Both branches are observed on chain — the sell's kcc20 args
// carry types=[0x02] as OP_2 and isMinters=[0x00] as `0100` in the same script.
function pushBytesMinimal(buf) {
  const b = buf instanceof Uint8Array ? buf : toBytes(buf);
  if (b.length === 0) return Uint8Array.from([0x00]); // OP_0
  if (b.length === 1) {
    if (b[0] >= 1 && b[0] <= 16) return Uint8Array.from([0x50 + b[0]]); // OP_1..OP_16
    if (b[0] === 0x81) return Uint8Array.from([0x4f]); // OP_1NEGATE
  }
  return pushData(b);
}
function requireXOnly(hex, label) {
  const b = toBytes(hex);
  if (b.length !== 32) throw new Error(`curve: ${label} must be a 32-byte x-only key`);
  return b;
}

// ── state block layout ────────────────────────────────────────────────────────
// Curve program: head 0x6b, a graduated flag, then a push of the token covenant
// id and a push of the 8-byte little-endian token reserve. The reserve is the
// only field that moves per trade; the covid pins which token this curve serves.
const CURVE_COVID_OFFSET = 4; // 0x6b, flag byte, 0x00, 0x20(push32) → covid at 4
const CURVE_COVID_LEN = 32;
const CURVE_RESERVE_OFFSET = 37; // 0x08(push8) at 36 → reserve at 37..45
const CURVE_GRADUATED_OFFSET = 2;

// Parse the immutable facts out of a curve program's state block. `reserve` is
// the token inventory the curve currently backs; `value` (the curve UTXO's KAS
// amount) is NOT in the program and must be supplied from the live UTXO.
export function parseCurveState(programHex) {
  const p = toBytes(programHex);
  if (p.length < CURVE_RESERVE_OFFSET + 8 || p[0] !== 0x6b) {
    throw new Error("curve: not a KRON curve v1 program");
  }
  return {
    tokenCovid: toHex(p.slice(CURVE_COVID_OFFSET, CURVE_COVID_OFFSET + CURVE_COVID_LEN)),
    reserve: readU64LE(p, CURVE_RESERVE_OFFSET),
    graduated: p[CURVE_GRADUATED_OFFSET] !== 0x00,
  };
}

// Splice a new reserve into the same curve program, producing the continuation
// program the trade must re-commit. Only the 8 reserve bytes change.
export function nextCurveProgram(parentProgramHex, newReserve) {
  const p = toBytes(parentProgramHex);
  if (p.length < CURVE_RESERVE_OFFSET + 8 || p[0] !== 0x6b) {
    throw new Error("curve: not a KRON curve v1 program");
  }
  const out = p.slice();
  writeU64LE(out, CURVE_RESERVE_OFFSET, newReserve);
  return toHex(out);
}

// A KCC20 covenant cell carries [owner 32B | identifier_type 1B | amount 8B LE |
// is_minter 1B]. The unguarded build seen on the KRON market opens at offset 0
// with a push32 of the owner; the guarded build wraps it one byte in. Locate the
// block rather than assume a build, then splice the four fields.
function locateCellStateBlock(p) {
  if (p.length >= 46 && p[0] === 0x20 && p[33] === 0x01 && p[35] === 0x08 && p[44] === 0x01) {
    return 0;
  }
  if (
    p.length >= 48 && p[0] === 0x6b && p[1] === 0x20 && p[34] === 0x01 &&
    p[36] === 0x08 && p[45] === 0x01 && p[47] === 0x6c
  ) {
    return 1;
  }
  throw new Error("curve: no KCC20 state block in cell program");
}
// Read the four state fields back out of a cell program. Used to prove a cell
// handed to the builder really is the inventory (or really is the seller's) —
// the balance a covenant input claims must come from its own revealed bytes,
// never from an API field.
export function parseCellState(cellProgramHex) {
  const p = toBytes(cellProgramHex);
  const start = locateCellStateBlock(p);
  return {
    owner: toHex(p.slice(start + 1, start + 33)),
    identifierType: p[start + 34],
    amount: readU64LE(p, start + 36),
    isMinter: p[start + 45],
  };
}

export function spliceCellState(cellBaseProgramHex, ownerHex, identifierType, amount, isMinter) {
  const base = toBytes(cellBaseProgramHex);
  const start = locateCellStateBlock(base);
  const owner = requireXOnly(ownerHex, "cell owner");
  const out = base.slice();
  out.set(owner, start + 1);
  out[start + 34] = identifierType & 0xff;
  writeU64LE(out, start + 36, amount);
  out[start + 45] = isMinter & 0xff;
  return toHex(out);
}

// ── script-public-key builders ────────────────────────────────────────────────
// P2SH: OP_BLAKE2B <32-byte program hash> OP_EQUAL, i.e. "aa20"+hash+"87".
export function p2shScriptPubKey(programHex) {
  return "aa20" + toHex(blake2b256(toBytes(programHex))) + "87";
}
// P2PK (schnorr): <32-byte x-only key> OP_CHECKSIG, i.e. "20"+key+"ac".
export function p2pkScriptPubKey(xOnlyKeyHex) {
  return "20" + toHex(requireXOnly(xOnlyKeyHex, "p2pk key")) + "ac";
}
// The P2SH script of a token cell holding a given state.
export function kcc20CellScriptPubKey(cellBaseProgramHex, ownerHex, identifierType, amount, isMinter) {
  return p2shScriptPubKey(spliceCellState(cellBaseProgramHex, ownerHex, identifierType, amount, isMinter));
}

// ── fee recipients, sourced from the program itself ───────────────────────────
// The three fee legs pay keys baked into the curve program. Creator is
// per-deployment; the platform/dev treasury is a KRON-global. Each key is
// inlined at more than one slot in the v1 skeleton, so we read one slot and
// require a second copy to agree — a shape we do not recognise fails closed
// rather than paying an unverified address. Offsets are pinned to KRON curve v1
// and proven against two real trades in curve.test.mjs.
const CREATOR_SLOTS = [191, 567];
const PLATFORM_SLOTS = [21013, 21073];
function readAgreedKey(p, slots, label) {
  const first = toHex(p.slice(slots[0], slots[0] + 32));
  for (const s of slots.slice(1)) {
    if (toHex(p.slice(s, s + 32)) !== first) {
      throw new Error(`curve: ${label} slots disagree — not a KRON curve v1 program`);
    }
  }
  return first;
}
export function feeKeysFromCurveProgram(programHex) {
  const p = toBytes(programHex);
  if (p.length < 21105 || p[0] !== 0x6b) throw new Error("curve: not a KRON curve v1 program");
  const platform = readAgreedKey(p, PLATFORM_SLOTS, "platform");
  return {
    creator: readAgreedKey(p, CREATOR_SLOTS, "creator"),
    platform,
    dev: platform, // the dev leg pays the same platform/dev treasury
  };
}

// ── the three fee legs ────────────────────────────────────────────────────────
// Fees are floored basis points of the KAS that moves; dev additionally floors
// at 0.2 KAS. Returned as sompi.
export function feeLegs(quoteSompi) {
  const q = BigInt(quoteSompi);
  const dev = (q * FEE_DEV_BPS) / BPS;
  return {
    creator: (q * FEE_CREATOR_BPS) / BPS,
    platform: (q * FEE_PLATFORM_BPS) / BPS,
    dev: dev > FEE_DEV_FLOOR_SOMPI ? dev : FEE_DEV_FLOOR_SOMPI,
  };
}

// ── the quotes (constant product) ─────────────────────────────────────────────
// (V + kasReserve) * tokenReserve is conserved; the covenant only lets that
// product grow (never shrink), so the executed side is rounded to keep it from
// shrinking. All arithmetic is exact BigInt.
function ceilDiv(a, b) { return (a + b - 1n) / b; }

// BUY: the buyer commits kasIn (a whole multiple of SCALE) and receives the most
// tokens that keep the product from shrinking. state.reserve is the token side;
// state.value is the curve UTXO's live KAS amount (sompi).
export function quoteBuy(state, kasInSompi) {
  const kasIn = BigInt(kasInSompi);
  const b0 = BigInt(state.reserve);
  const k0 = BigInt(state.value);
  if (kasIn <= 0n) throw new Error("curve: kasIn must be positive");
  if (kasIn % SCALE !== 0n) throw new Error("curve: kasIn must be a whole multiple of SCALE");
  if (b0 <= 0n) throw new Error("curve: empty curve reserve");
  const k1 = k0 + kasIn;
  // b1 = ceil((V+k0)*b0 / (V+k1)); tokenOut = b0 - b1.
  const b1 = ceilDiv((V_SOMPI + k0) * b0, V_SOMPI + k1);
  const tokenOut = b0 - b1;
  if (tokenOut <= 0n) throw new Error("curve: kasIn too small to buy a whole token");
  return {
    tokenOut,
    newReserve: b1,
    curveValueAfter: k1,
    fees: feeLegs(kasIn),
  };
}

// SELL: the seller delivers tokenIn tokens to inventory and receives the most
// KAS that keeps the product from shrinking, floored to a whole multiple of
// SCALE. The seller must deliver exactly one whole token cell of `tokenIn` — the
// covenant leaves no seller token change on this branch.
export function quoteSell(state, tokenIn) {
  const dt = BigInt(tokenIn);
  const b0 = BigInt(state.reserve);
  const k0 = BigInt(state.value);
  if (dt <= 0n) throw new Error("curve: tokenIn must be positive");
  if (b0 <= 0n) throw new Error("curve: empty curve reserve");
  const b1 = b0 + dt;
  // k1min = ceil((V+k0)*b0 / b1) - V; maxKasOut = k0 - k1min; floor to SCALE.
  const k1min = ceilDiv((V_SOMPI + k0) * b0, b1) - V_SOMPI;
  const maxKasOut = k0 - k1min;
  if (maxKasOut <= 0n) throw new Error("curve: tokenIn too small to release any KAS");
  const kasOut = (maxKasOut / SCALE) * SCALE;
  if (kasOut <= 0n) throw new Error("curve: proceeds below one SCALE step");
  return {
    kasOut,
    newReserve: b1,
    curveValueAfter: k0 - kasOut,
    fees: feeLegs(kasOut),
  };
}

// ── the ordered covenant pushes (curve input signatureScript) ─────────────────
// The curve leg carries no signature: twelve pushes then the program reveal.
// buildBuyArgs returns them as { pushes: [hex…], signatureScript: hex }, ready to
// drop in as input-0's signatureScript. Order is consensus-critical.
export function buildBuyArgs({
  parentProgramHex,
  marketCovenantId,
  kasInSompi,
  tokenOut,
  newReserve, // the market's inventory AND the curve reserve after the buy (= b1)
  buyerXOnlyKey,
}) {
  const C = requireXOnly(marketCovenantId, "market covenant id");
  const buyer = requireXOnly(buyerXOnlyKey, "buyer key");
  const pushes = [
    pushNumber(kasInSompi),                     // kasIn (LE min-int)
    pushNumber(tokenOut),                       // tokenOut
    pushData(C),                                // inventory owner = covenant id
    Uint8Array.from([0x52]),                    // inventory type = OP_2
    pushNumber(newReserve),                     // inventory amount after
    Uint8Array.from([0x00]),                    // inventory minter = OP_0
    pushData(buyer),                            // buyer owner
    Uint8Array.from([0x53]),                    // buyer type = OP_3
    pushNumber(tokenOut),                       // buyer amount
    Uint8Array.from([0x00]),                    // buyer minter = OP_0
    pushNumber(SELECTOR_BUY),                   // selector = OP_1
    pushData(parentProgramHex),                 // program reveal
  ];
  return serializeArgs(pushes);
}

export function buildSellArgs({
  parentProgramHex,
  marketCovenantId,
  tokenIn, // unused in the pushes directly but kept for caller symmetry/validation
  kasOutSompi,
  newReserve, // the market's inventory AND the curve reserve after the sell (= b1)
  sellerXOnlyKey,
  changeAmount = 1n, // seller token-change placeholder; observed as OP_1 on chain
}) {
  const C = requireXOnly(marketCovenantId, "market covenant id");
  const seller = requireXOnly(sellerXOnlyKey, "seller key");
  void tokenIn;
  const pushes = [
    pushNumber(tokenIn),                        // tokenIn
    pushNumber(kasOutSompi),                    // kasOut (LE min-int)
    pushData(C),                                // inventory owner = covenant id
    Uint8Array.from([0x52]),                    // inventory type = OP_2
    pushNumber(newReserve),                     // inventory amount after
    Uint8Array.from([0x00]),                    // inventory minter = OP_0
    pushData(seller),                           // change owner = seller
    Uint8Array.from([0x53]),                    // change type = OP_3
    pushNumber(changeAmount),                   // change amount
    Uint8Array.from([0x00]),                    // change minter = OP_0
    pushNumber(SELECTOR_SELL),                  // selector = OP_2
    pushData(parentProgramHex),                 // program reveal
  ];
  return serializeArgs(pushes);
}

function serializeArgs(pushes) {
  return {
    pushes: pushes.map(toHex),
    signatureScript: toHex(concat(pushes)),
  };
}

// ── the KCC-20 transfer args (every kcc20 cell input's signatureScript) ───────
// Seven pushes, no signature. The new cell states are transposed into FOUR
// per-field arrays (all owners ‖ all types ‖ all amounts ‖ all isMinters) rather
// than four-field records; `sigs` is empty because nothing here is authorized by
// a key; `witnesses` names, for each kcc20 cell input of the transaction in
// order, the transaction-input index that authorizes it. Under kcc20.sil only
// identifier type 0x00 is authorized by a signature — 0x01/0x02/0x03 are
// authorized by a CO-PRESENT input, which is what this array points at:
//   a covenant-id-owned cell (type 0x02, the market inventory) points at the
//   curve input, whose covenant id equals the cell's owner;
//   a presence-owned cell (type 0x03, a holder balance) points at the P2PK
//   input carrying that holder's key.
// EVERY kcc20 input of one transaction carries the SAME seven args except the
// last: each reveals its own parent program. Layout parsed out of, and rebuilt
// byte-for-byte against, the real trades' 2,485/2,527-byte kcc20 sigscripts.
export function buildKcc20TransferArgs({ newStates, witnesses, parentProgramHex }) {
  if (!Array.isArray(newStates) || newStates.length === 0) {
    throw new Error("curve: kcc20 transfer needs at least one new cell state");
  }
  if (!Array.isArray(witnesses) || witnesses.length === 0) {
    throw new Error("curve: kcc20 transfer needs a witness index per kcc20 input");
  }
  const owners = [];
  const types = [];
  const amounts = [];
  const minters = [];
  for (const s of newStates) {
    owners.push(requireXOnly(s.owner, "kcc20 new-state owner"));
    const t = Number(s.identifierType);
    if (!Number.isInteger(t) || t < 0 || t > 0xff) throw new Error("curve: kcc20 identifierType out of range");
    types.push(Uint8Array.from([t]));
    const a = new Uint8Array(8);
    writeU64LE(a, 0, s.amount);
    amounts.push(a);
    minters.push(Uint8Array.from([s.isMinter ? 1 : 0]));
  }
  const w = new Uint8Array(witnesses.length);
  for (let i = 0; i < witnesses.length; i++) {
    const idx = Number(witnesses[i]);
    // one byte per entry: a transaction with more than 256 inputs cannot be
    // addressed by this encoding, and guessing a wider one is not an option.
    if (!Number.isInteger(idx) || idx < 0 || idx > 0xff) {
      throw new Error("curve: kcc20 witness index out of the single-byte range");
    }
    w[i] = idx;
  }
  const pushes = [
    pushBytesMinimal(concat(owners)),   // owners ‖
    pushBytesMinimal(concat(types)),    // identifier types ‖
    pushBytesMinimal(concat(amounts)),  // amounts, 8B LE each ‖
    pushBytesMinimal(concat(minters)),  // isMinter flags
    pushBytesMinimal(new Uint8Array(0)),// sigs: empty — no key authorizes this
    pushBytesMinimal(w),                // witnesses: authorizing input per kcc20 input
    pushData(parentProgramHex),         // this input's own program reveal
  ];
  return serializeArgs(pushes);
}

// ── the full output list ──────────────────────────────────────────────────────
// Every consensus-enforced output, in order. out0..outN-2 are fixed by the
// trade; the final change output is free money back to the user and its value is
// whatever the funding leaves after fees, dust and the network fee — the caller
// supplies it.
export function assembleBuyOutputs({
  parentProgramHex,
  cellBaseProgramHex,
  marketCovenantId,
  quote, // from quoteBuy
  buyerXOnlyKey,
  creatorKey,
  platformKey,
  devKey,
  changeSompi,
}) {
  const b1 = BigInt(quote.newReserve);
  return [
    { value: BigInt(quote.curveValueAfter), scriptPublicKey: p2shScriptPubKey(nextCurveProgram(parentProgramHex, b1)) },
    { value: CELL_DUST_SOMPI, scriptPublicKey: kcc20CellScriptPubKey(cellBaseProgramHex, marketCovenantId, CELL_TYPE_INVENTORY, b1, 0) },
    { value: CELL_DUST_SOMPI, scriptPublicKey: kcc20CellScriptPubKey(cellBaseProgramHex, buyerXOnlyKey, CELL_TYPE_HOLDER, BigInt(quote.tokenOut), 0) },
    { value: quote.fees.creator, scriptPublicKey: p2pkScriptPubKey(creatorKey) },
    { value: quote.fees.platform, scriptPublicKey: p2pkScriptPubKey(platformKey) },
    { value: quote.fees.dev, scriptPublicKey: p2pkScriptPubKey(devKey) },
    { value: BigInt(changeSompi), scriptPublicKey: p2pkScriptPubKey(buyerXOnlyKey) },
  ];
}

export function assembleSellOutputs({
  parentProgramHex,
  cellBaseProgramHex,
  marketCovenantId,
  quote, // from quoteSell
  sellerXOnlyKey,
  creatorKey,
  platformKey,
  devKey,
  refundSompi,
}) {
  const b1 = BigInt(quote.newReserve);
  return [
    { value: BigInt(quote.curveValueAfter), scriptPublicKey: p2shScriptPubKey(nextCurveProgram(parentProgramHex, b1)) },
    { value: CELL_DUST_SOMPI, scriptPublicKey: kcc20CellScriptPubKey(cellBaseProgramHex, marketCovenantId, CELL_TYPE_INVENTORY, b1, 0) },
    { value: quote.fees.creator, scriptPublicKey: p2pkScriptPubKey(creatorKey) },
    { value: quote.fees.platform, scriptPublicKey: p2pkScriptPubKey(platformKey) },
    { value: quote.fees.dev, scriptPublicKey: p2pkScriptPubKey(devKey) },
    { value: BigInt(refundSompi), scriptPublicKey: p2pkScriptPubKey(sellerXOnlyKey) },
  ];
}

// ═════════════════════════════════════════════════════════════════════════════
// THE ASSEMBLY LAYER — buildBuy / buildSell
// ═════════════════════════════════════════════════════════════════════════════
//
// Everything above builds pieces; this builds the transaction the page hands to
// the wallet. It stays pure and offline: no fetch, no SDK import, no wallet. The
// caller injects the two things that need the outside world — a mass function
// and (optionally) an address→script resolver.
//
// The user's wallet signs ONE thing: the user's own P2PK funding inputs. Every
// covenant input's signatureScript is built here and carries no signature, so
// there is nothing for the wallet to sign on the curve or the cells; `signInputs`
// names only the funding indexes and wallet.js proves the wallet touched nothing
// else.
//
// Proven input layout (both real trades, reconstructed byte-for-byte):
//   BUY  [0] curve cell, [1] inventory kcc20 cell, [2…] user funding (P2PK)
//   SELL [0] curve cell, [1] inventory kcc20 cell, [2] seller token cell,
//        [3…] seller funding (P2PK)

// Per-input compute budget, as the node reported it for both real trades
// (curve 400, each kcc20 cell 100, each P2PK 10). It is carried on the wire but
// is NOT part of the transaction id, so the id match in assembly.test.mjs does
// not prove it — the values come from the node's own decode of the two trades.
export const COMPUTE_BUDGET_CURVE = 400;
export const COMPUTE_BUDGET_CELL = 100;
export const COMPUTE_BUDGET_P2PK = 10;

// Declared sig ops. A covenant input carries no signature; a P2PK input carries
// exactly one. Like the compute budget this is outside the transaction id, so
// the fixtures cannot confirm it (api.kaspa.org reports sig_op_count as null for
// these trades) — it is the only field of the built transaction with no chain
// evidence behind it.
/* A version-1 input commits a compute budget INSTEAD of the v0 sig-op count
 * (kascov's own decoder says so in kascov-core/src/model.rs: "v1 per-input
 * compute budget commitment (replaces v0 sig_op_count)"). Carrying a non-zero
 * legacy count on a v1 transaction is what the node means by
 * "sig_op_count is inconsistent with transaction version 1" — it rejected a
 * real trade over exactly this. Every v1 input therefore states 0 here and
 * lets computeBudget speak. */
export const SIGOPS_COVENANT = 0;
export const SIGOPS_P2PK = 0;

// NETWORK FEE. The two proven trades paid 42_124_320 sompi (buy, node-reported
// mass 229_134) and 42_714_960 (sell, mass 241_159) and both confirmed on
// mainnet. The vendored SDK's calculateTransactionMass does NOT reproduce those
// masses — it says 244_473 for the buy and 180_347 for the sell, understating
// the sell by ~25% — so a mass-derived fee is a LOWER BOUND here and nothing
// more. The default is therefore a flat floor above what both proven trades
// paid, and the mass-derived figure only ever raises it. The cap exists so a
// bad mass or a fat-fingered override cannot quietly eat the wallet.
export const NETWORK_FEE_FLOOR_SOMPI = 50_000_000n; // 0.5 KAS
export const NETWORK_FEE_CAP_SOMPI = 200_000_000n; // 2 KAS, hard refusal above
export const FEE_RATE_SOMPI_PER_GRAM = 1n; // network minimum relay rate
export const FEE_MASS_HEADROOM = 4n; // multiplier on the mass-derived figure

// A change output smaller than this is not worth creating: Kaspa's storage mass
// penalises tiny outputs, so the trade would cost more than the change is worth.
// The builder refuses rather than silently donating the remainder to fees.
export const MIN_CHANGE_SOMPI = 20_000_000n; // 0.2 KAS

const HEX64_RE = /^[0-9a-f]{64}$/;

function normOutpoint(src, label) {
  const op = (src && src.outpoint) || src;
  const txid = String((op && (op.transactionId ?? op.transaction_id)) ?? "").toLowerCase();
  const index = op && op.index;
  if (!HEX64_RE.test(txid)) throw new Error(`curve: ${label} has no 32-byte transaction id`);
  if (!Number.isInteger(Number(index)) || Number(index) < 0) {
    throw new Error(`curve: ${label} has no output index`);
  }
  return { transactionId: txid, index: Number(index) };
}
function scriptHexOf(spk) {
  if (spk == null) return null;
  if (typeof spk === "string") return spk.toLowerCase();
  const s = spk.script ?? spk.scriptPublicKey ?? spk.script_public_key;
  return s == null ? null : String(s).toLowerCase();
}
function amountOf(u, label) {
  const v = u && (u.amountSompi ?? u.valueSompi ?? u.amount ?? u.value);
  if (v == null) throw new Error(`curve: ${label} carries no amount`);
  return BigInt(v);
}
function normHex(raw, label) {
  const h = String(raw ?? "").trim().toLowerCase().replace(/^0x/, "");
  if (h.length % 2 !== 0 || /[^0-9a-f]/.test(h)) throw new Error(`curve: ${label} is not hex`);
  return h;
}

// Enumerate a signatureScript as pushes/opcodes. verify() re-reads the built
// bytes with this rather than trusting the values the builder started from.
export function parseScriptPushes(hex) {
  const b = toBytes(hex);
  const out = [];
  let i = 0;
  while (i < b.length) {
    const op = b[i];
    let ds;
    let len;
    if (op >= 0x01 && op <= 0x4b) { ds = i + 1; len = op; }
    else if (op === 0x4c) { len = b[i + 1]; ds = i + 2; }
    else if (op === 0x4d) { len = b[i + 1] | (b[i + 2] << 8); ds = i + 3; }
    else if (op === 0x4e) { len = b[i + 1] | (b[i + 2] << 8) | (b[i + 3] << 16) | (b[i + 4] * 16777216); ds = i + 5; }
    else { out.push({ opcode: op, data: null }); i += 1; continue; }
    if (ds + len > b.length) throw new Error("curve: truncated push in signatureScript");
    out.push({ opcode: null, data: b.slice(ds, ds + len) });
    i = ds + len;
  }
  return out;
}
// A numeric arg: OP_0, OP_1..OP_16, or a minimal little-endian scriptNum push.
function argToBigInt(p) {
  if (p.data === null) {
    if (p.opcode === 0x00) return 0n;
    if (p.opcode >= 0x51 && p.opcode <= 0x60) return BigInt(p.opcode - 0x50);
    throw new Error(`curve: opcode 0x${p.opcode.toString(16)} is not a number`);
  }
  const d = p.data;
  if (d.length === 0) return 0n;
  let v = 0n;
  for (let i = d.length - 1; i >= 0; i--) v = (v << 8n) | BigInt(d[i]);
  const signBit = 1n << BigInt(8 * d.length - 1);
  return v & signBit ? -(v ^ signBit) : v;
}
// The byte-array form of an arg (OP_N and OP_0 re-expanded), for the kcc20 args
// whose fields are arrays, not numbers.
function argToBytes(p) {
  if (p.data !== null) return p.data;
  if (p.opcode === 0x00) return new Uint8Array(0);
  if (p.opcode >= 0x51 && p.opcode <= 0x60) return Uint8Array.from([p.opcode - 0x50]);
  throw new Error(`curve: opcode 0x${p.opcode.toString(16)} is not data`);
}

// ── the validated inputs a build starts from ──────────────────────────────────

// Refuses, loudly, anything the two proven trades do not cover: an unrecognised
// market build, a curve that is mid-trade (more than one live cell), a program
// that does not hash to the cell it claims to spend, a reserve that disagrees
// with the revealed program.
function normalizeCurveState(state) {
  if (!state || typeof state !== "object") throw new Error("curve: no curve state supplied");
  if (state.skeletonSupported !== true) {
    throw new Error(
      "curve: the live market build is not the skeleton this builder was proven against " +
        "(state.skeletonSupported is not true) — refusing to build a trade blind",
    );
  }
  const live = state.liveUtxo || {};
  const count = live.count ?? live.live_count ?? null;
  if (count != null && Number(count) !== 1) {
    throw new Error(
      `curve: the curve shows ${count} live cells; a trade may only be built against exactly one`,
    );
  }
  if (!state.programHex) throw new Error("curve: state carries no curve program to reveal");
  const programHex = normHex(state.programHex, "curve program");
  const marketCovenantId = normHex(state.marketCovenantId || state.marketXOnly, "market covenant id");
  requireXOnly(marketCovenantId, "market covenant id");

  const outpoint = normOutpoint(live, "curve live cell");
  const value = BigInt(live.valueSompi ?? state.value);
  const reserve = BigInt(state.reserve);
  if (state.value != null && live.valueSompi != null && BigInt(state.value) !== value) {
    throw new Error("curve: state.value disagrees with the live cell's value");
  }

  // The revealed program is the source of truth for the reserve and the token.
  const parsed = parseCurveState(programHex);
  if (parsed.reserve !== reserve) {
    throw new Error(
      `curve: state.reserve (${reserve}) is not the reserve committed by the program (${parsed.reserve})`,
    );
  }
  if (parsed.graduated) throw new Error("curve: this curve has graduated and no longer bonds");

  // …and it must be the program the live cell actually commits to.
  const committed = scriptHexOf(live.scriptPublicKey);
  const derived = p2shScriptPubKey(programHex);
  if (committed && committed !== derived) {
    throw new Error("curve: the curve program does not hash to the live cell's committed script");
  }
  return {
    programHex,
    marketCovenantId,
    tokenCovid: parsed.tokenCovid,
    reserve,
    value,
    outpoint,
    scriptPublicKey: derived,
  };
}

// A kcc20 cell the trade will spend: its program must hash to what it claims to
// commit to, and its state head must say what the caller says it says.
function normalizeCell(cell, label, expect) {
  if (!cell || typeof cell !== "object") throw new Error(`curve: no ${label} supplied`);
  const programHex = normHex(cell.programHex || cell.program_hex, `${label} program`);
  const outpoint = normOutpoint(cell, label);
  const value = amountOf(cell, label);
  const derived = p2shScriptPubKey(programHex);
  const committed = scriptHexOf(cell.scriptPublicKey || cell.script_hex);
  if (committed && committed !== derived) {
    throw new Error(`curve: the ${label} program does not hash to its committed script`);
  }
  const st = parseCellState(programHex);
  if (expect.owner && st.owner !== expect.owner) {
    throw new Error(`curve: the ${label} is owned by ${st.owner}, not ${expect.owner}`);
  }
  if (expect.identifierType != null && st.identifierType !== expect.identifierType) {
    throw new Error(
      `curve: the ${label} has identifier type ${st.identifierType}, not ${expect.identifierType}`,
    );
  }
  if (st.isMinter !== 0) throw new Error(`curve: the ${label} is a minter cell; not a trade cell`);
  if (value !== CELL_DUST_SOMPI) {
    throw new Error(`curve: the ${label} carries ${value} sompi, not the ${CELL_DUST_SOMPI} dust floor`);
  }
  return { programHex, outpoint, value, scriptPublicKey: derived, state: st };
}

// Funding must be the user's own P2PK, because it is the only thing the wallet
// will be asked to sign and (on a sell) the only thing that authorizes the
// seller's presence-owned token cell.
function normalizeFunding(utxos, userXOnly) {
  if (!Array.isArray(utxos) || utxos.length === 0) {
    throw new Error("curve: no funding UTXOs supplied");
  }
  const spk = p2pkScriptPubKey(userXOnly);
  const seen = new Set();
  return utxos.map((u, i) => {
    const outpoint = normOutpoint(u, `funding utxo ${i}`);
    const key = `${outpoint.transactionId}:${outpoint.index}`;
    if (seen.has(key)) throw new Error(`curve: funding utxo ${key} listed twice`);
    seen.add(key);
    const committed = scriptHexOf(u.scriptPublicKey);
    if (committed && committed !== spk) {
      throw new Error(
        `curve: funding utxo ${key} does not pay the user's P2PK key; only the user's own ` +
          "P2PK outputs may fund a trade",
      );
    }
    return { outpoint, amount: amountOf(u, `funding utxo ${i}`), scriptPublicKey: spk };
  });
}

// Largest first. `needed` may be zero or negative on a sell (the proceeds cover
// the costs) — a funding input is still mandatory there, because the seller's
// token cell is authorized by a co-present P2PK input, not by a signature.
function selectFunding(pool, needed, minCount) {
  const sorted = [...pool].sort((a, b) => (a.amount < b.amount ? 1 : a.amount > b.amount ? -1 : 0));
  const picked = [];
  let sum = 0n;
  for (const u of sorted) {
    if (picked.length >= minCount && sum >= needed) break;
    picked.push(u);
    sum += u.amount;
  }
  if (picked.length < minCount || sum < needed) {
    throw new Error(
      `curve: funding is short — need ${needed} sompi across at least ${minCount} input(s), ` +
        `the supplied UTXOs total ${sum}`,
    );
  }
  return { picked, sum };
}

// The fee the transaction will pay. An explicit feeSompi wins outright (that is
// how the acceptance gate replays a real trade); otherwise the floor, raised —
// never lowered — by the mass-derived figure when a mass function was injected.
function resolveNetworkFee({ feeSompi, mass, feeRateSompiPerGram, minNetworkFeeSompi, maxNetworkFeeSompi }) {
  const cap = maxNetworkFeeSompi != null ? BigInt(maxNetworkFeeSompi) : NETWORK_FEE_CAP_SOMPI;
  if (feeSompi != null) {
    const f = BigInt(feeSompi);
    if (f <= 0n) throw new Error("curve: feeSompi must be positive — a zero-fee transaction is not relayed");
    if (f > cap) throw new Error(`curve: feeSompi ${f} exceeds the ${cap} sompi safety cap`);
    return { fee: f, source: "caller", cap };
  }
  const floor = minNetworkFeeSompi != null ? BigInt(minNetworkFeeSompi) : NETWORK_FEE_FLOOR_SOMPI;
  const rate = feeRateSompiPerGram != null ? BigInt(feeRateSompiPerGram) : FEE_RATE_SOMPI_PER_GRAM;
  const fromMass = mass != null ? BigInt(mass) * rate * FEE_MASS_HEADROOM : 0n;
  const fee = fromMass > floor ? fromMass : floor;
  if (fee > cap) throw new Error(`curve: computed fee ${fee} exceeds the ${cap} sompi safety cap`);
  return { fee, source: mass != null && fromMass > floor ? "mass" : "floor", cap, mass, rate };
}

function txInput(outpoint, scriptPublicKey, valueSompi, signatureScript, computeBudget, sigOpCount) {
  return {
    previousOutpoint: outpoint,
    signatureScript,
    sequence: 0n,
    sigOpCount,
    computeBudget,
    utxo: {
      outpoint,
      amount: valueSompi,
      scriptPublicKey: { version: 0, script: scriptPublicKey },
      blockDaaScore: 0n,
      isCoinbase: false,
    },
  };
}
function txOutput(value, scriptHex, covenant) {
  const o = { value, scriptPublicKey: { version: 0, script: scriptHex } };
  if (covenant) o.covenant = covenant;
  return o;
}

// ── buildBuy ──────────────────────────────────────────────────────────────────

export function buildBuy(state, params = {}) {
  return assembleTrade("buy", state, params);
}

// ── buildSell ─────────────────────────────────────────────────────────────────

export function buildSell(state, params = {}) {
  return assembleTrade("sell", state, params);
}

function assembleTrade(kind, rawState, params) {
  const st = normalizeCurveState(rawState);
  const isBuy = kind === "buy";
  const userXOnly = normHex(
    isBuy ? params.buyerXOnlyPubkey : params.sellerXOnlyPubkey,
    isBuy ? "buyerXOnlyPubkey" : "sellerXOnlyPubkey",
  );
  requireXOnly(userXOnly, isBuy ? "buyer key" : "seller key");

  // The inventory cell the market spends on every trade. Its committed balance
  // IS the curve's reserve; if the two disagree the page is looking at a stale
  // cell and must not build.
  const inventory = normalizeCell(params.inventoryCell, "inventory cell", {
    owner: st.marketCovenantId,
    identifierType: 0x02,
  });
  if (inventory.state.amount !== st.reserve) {
    throw new Error(
      `curve: the inventory cell holds ${inventory.state.amount} but the curve commits to ` +
        `${st.reserve} — refusing to build against a stale inventory cell`,
    );
  }

  // The seller's token cells. Partial-cell sells are NOT proven by either
  // fixture (the real sell consumed one cell whole and left no token change),
  // and a second seller cell would need a witness layout no trade has shown, so
  // both are refused rather than guessed.
  let tokenCells = [];
  let tokenIn = 0n;
  if (!isBuy) {
    const raw = params.tokenCells;
    if (!Array.isArray(raw) || raw.length === 0) throw new Error("curve: a sell needs a token cell");
    if (raw.length > 1) {
      throw new Error(
        "curve: selling more than one token cell in a transaction is not proven by any observed " +
          "trade (the witness layout for a second presence-owned cell is unverified) — sell one cell at a time",
      );
    }
    tokenCells = raw.map((c, i) =>
      normalizeCell(c, `token cell ${i}`, { owner: userXOnly, identifierType: 0x03 }),
    );
    tokenIn = tokenCells.reduce((s, c) => s + c.state.amount, 0n);
  }

  // The quote, and the exactness gate on what the caller asked for.
  let quote;
  if (isBuy) {
    if (params.kasInSompi == null) throw new Error("curve: a buy needs kasInSompi");
    const kasIn = BigInt(params.kasInSompi);
    if (kasIn % SCALE !== 0n) {
      throw new Error(`curve: kasIn ${kasIn} is not a whole multiple of SCALE (${SCALE})`);
    }
    quote = quoteBuy({ reserve: st.reserve, value: st.value }, kasIn);
    quote.kasIn = kasIn;
  } else {
    quote = quoteSell({ reserve: st.reserve, value: st.value }, tokenIn);
    quote.tokenIn = tokenIn;
    if (params.kasOutSompi != null) {
      const want = BigInt(params.kasOutSompi);
      if (want % SCALE !== 0n) {
        throw new Error(`curve: kasOut ${want} is not a whole multiple of SCALE (${SCALE})`);
      }
      if (want !== quote.kasOut) {
        throw new Error(
          `curve: the supplied token cells deliver ${tokenIn} tokens, which the curve prices at ` +
            `${quote.kasOut} sompi, not the requested ${want} — no cell combination delivers exactly ` +
            "that, and partial-cell sells are not supported",
        );
      }
    }
  }

  // Crossing the graduation target ends bonding, and no observed trade shows
  // what the covenant does on the crossing block. Refuse rather than find out
  // with the trader's money.
  if (quote.curveValueAfter > GRADUATION_KAS_SOMPI) {
    throw new Error(
      `curve: this trade would carry the reserve to ${quote.curveValueAfter} sompi, past the ` +
        `${GRADUATION_KAS_SOMPI} graduation target; no observed trade proves that crossing — ` +
        "trade a smaller size",
    );
  }

  const feeKeys = feeKeysFromCurveProgram(st.programHex);
  const b1 = quote.newReserve;

  // Change pays the user's own key by construction. An explicitly supplied
  // script is honoured only if it is that same script — there is no path here
  // that routes the user's change anywhere else.
  const userP2pk = p2pkScriptPubKey(userXOnly);
  let changeScript = userP2pk;
  if (params.changeScriptPublicKey != null) {
    changeScript = normHex(scriptHexOf(params.changeScriptPublicKey), "changeScriptPublicKey");
    if (changeScript !== userP2pk) {
      throw new Error("curve: change may only pay the trader's own P2PK key");
    }
  }
  if (params.changeAddress != null) {
    if (typeof params.addressToScript !== "function") {
      throw new Error(
        "curve: changeAddress was supplied without an addressToScript resolver; the builder will " +
          "not assume an address decodes to the trader's own key",
      );
    }
    const resolved = normHex(scriptHexOf(params.addressToScript(params.changeAddress)), "changeAddress script");
    if (resolved !== userP2pk) {
      throw new Error("curve: changeAddress does not decode to the trader's own P2PK key");
    }
  }

  // Covenant sigscripts. Neither carries a signature.
  const curveArgs = isBuy
    ? buildBuyArgs({
        parentProgramHex: st.programHex,
        marketCovenantId: st.marketCovenantId,
        kasInSompi: quote.kasIn,
        tokenOut: quote.tokenOut,
        newReserve: b1,
        buyerXOnlyKey: userXOnly,
      })
    : buildSellArgs({
        parentProgramHex: st.programHex,
        marketCovenantId: st.marketCovenantId,
        tokenIn,
        kasOutSompi: quote.kasOut,
        newReserve: b1,
        sellerXOnlyKey: userXOnly,
        // Not a knob. The observed sell pushes OP_1 here and produces no seller
        // token-change output, so 1 is the only value with chain evidence; the
        // whole-cell gate above is what makes it correct.
        changeAmount: 1n,
      });

  const newStates = isBuy
    ? [
        { owner: st.marketCovenantId, identifierType: 0x02, amount: b1, isMinter: 0 },
        { owner: userXOnly, identifierType: 0x03, amount: quote.tokenOut, isMinter: 0 },
      ]
    : [{ owner: st.marketCovenantId, identifierType: 0x02, amount: b1, isMinter: 0 }];

  // Input indexes are fixed by the proven layout.
  const CURVE_IDX = 0;
  const INVENTORY_IDX = 1;
  const firstFundingIdx = isBuy ? 2 : 2 + tokenCells.length;
  // The inventory is covenant-id-owned (type 0x02) and is authorized by the
  // curve input, whose covenant id equals its owner. A seller cell is
  // presence-owned (type 0x03) and is authorized by the co-present P2PK input
  // carrying the seller's key — the first funding input.
  const witnesses = isBuy ? [CURVE_IDX] : [CURVE_IDX, ...tokenCells.map(() => firstFundingIdx)];

  const inventoryArgs = buildKcc20TransferArgs({
    newStates,
    witnesses,
    parentProgramHex: inventory.programHex,
  });
  const tokenCellArgs = tokenCells.map((c) =>
    buildKcc20TransferArgs({ newStates, witnesses, parentProgramHex: c.programHex }),
  );

  // Fixed (non-change) outputs.
  const fixedOutputs = isBuy
    ? [
        txOutput(quote.curveValueAfter, p2shScriptPubKey(nextCurveProgram(st.programHex, b1)), {
          authorizingInput: CURVE_IDX,
          covenantId: st.marketCovenantId,
        }),
        txOutput(CELL_DUST_SOMPI, kcc20CellScriptPubKey(inventory.programHex, st.marketCovenantId, 0x02, b1, 0), {
          authorizingInput: INVENTORY_IDX,
          covenantId: st.tokenCovid,
        }),
        txOutput(CELL_DUST_SOMPI, kcc20CellScriptPubKey(inventory.programHex, userXOnly, 0x03, quote.tokenOut, 0), {
          authorizingInput: INVENTORY_IDX,
          covenantId: st.tokenCovid,
        }),
        txOutput(quote.fees.creator, p2pkScriptPubKey(feeKeys.creator)),
        txOutput(quote.fees.platform, p2pkScriptPubKey(feeKeys.platform)),
        txOutput(quote.fees.dev, p2pkScriptPubKey(feeKeys.dev)),
      ]
    : [
        txOutput(quote.curveValueAfter, p2shScriptPubKey(nextCurveProgram(st.programHex, b1)), {
          authorizingInput: CURVE_IDX,
          covenantId: st.marketCovenantId,
        }),
        txOutput(CELL_DUST_SOMPI, kcc20CellScriptPubKey(inventory.programHex, st.marketCovenantId, 0x02, b1, 0), {
          authorizingInput: INVENTORY_IDX,
          covenantId: st.tokenCovid,
        }),
        txOutput(quote.fees.creator, p2pkScriptPubKey(feeKeys.creator)),
        txOutput(quote.fees.platform, p2pkScriptPubKey(feeKeys.platform)),
        txOutput(quote.fees.dev, p2pkScriptPubKey(feeKeys.dev)),
      ];

  const feeTotal = quote.fees.creator + quote.fees.platform + quote.fees.dev;
  // What the user's funding must cover before the network fee. On a buy: the
  // KAS bought, the three fee legs and the 0.5 KAS dust of the buyer's NEW cell
  // (the inventory's dust passes straight through). On a sell the proceeds and
  // the reclaimed cell dust flow the other way, so this is usually negative.
  const covenantInValue = st.value + inventory.value + tokenCells.reduce((s, c) => s + c.value, 0n);
  const fixedOutValue = fixedOutputs.reduce((s, o) => s + o.value, 0n);
  const beforeFee = fixedOutValue - covenantInValue; // funding needed, fee excluded

  const pool = normalizeFunding(params.fundingUtxos, userXOnly);
  const payload = params.payload != null ? normHex(params.payload, "payload") : "";
  const network = params.network || "mainnet";
  const calcMass = typeof params.calculateTransactionMass === "function" ? params.calculateTransactionMass : null;

  // Fee and funding are mutually dependent (more inputs -> more mass -> more
  // fee -> maybe more inputs). Iterate to a fixed point; refuse rather than
  // ship a transaction whose fee was never re-checked against its final shape.
  let feeInfo = resolveNetworkFee({
    feeSompi: params.feeSompi,
    mass: null,
    feeRateSompiPerGram: params.feeRateSompiPerGram,
    minNetworkFeeSompi: params.minNetworkFeeSompi,
    maxNetworkFeeSompi: params.maxNetworkFeeSompi,
  });
  let built = null;
  for (let round = 0; round < 6; round++) {
    const needed = beforeFee + feeInfo.fee;
    const { picked, sum } = selectFunding(pool, needed > 0n ? needed : 0n, 1);
    const change = sum - needed;
    // Zero is refused with everything else below the floor: dropping the change
    // output would change the output count, and a shape with no change leg is
    // not one any observed trade proves.
    if (change < MIN_CHANGE_SOMPI) {
      throw new Error(
        `curve: the change output would be ${change} sompi, below the ${MIN_CHANGE_SOMPI} floor this ` +
          "builder will create; adjust the trade size or the funding UTXOs",
      );
    }
    const inputs = [
      txInput(st.outpoint, st.scriptPublicKey, st.value, curveArgs.signatureScript, COMPUTE_BUDGET_CURVE, SIGOPS_COVENANT),
      txInput(inventory.outpoint, inventory.scriptPublicKey, inventory.value, inventoryArgs.signatureScript, COMPUTE_BUDGET_CELL, SIGOPS_COVENANT),
      ...tokenCells.map((c, i) =>
        txInput(c.outpoint, c.scriptPublicKey, c.value, tokenCellArgs[i].signatureScript, COMPUTE_BUDGET_CELL, SIGOPS_COVENANT),
      ),
      // funding inputs go out UNSIGNED; the wallet fills these and nothing else
      ...picked.map((u) => txInput(u.outpoint, u.scriptPublicKey, u.amount, "", COMPUTE_BUDGET_P2PK, SIGOPS_P2PK)),
    ];
    const outputs = [...fixedOutputs, txOutput(change, changeScript)];
    const transaction = {
      version: 1,
      inputs,
      outputs,
      lockTime: 0n,
      subnetworkId: "0000000000000000000000000000000000000000",
      gas: 0n,
      payload,
    };
    built = { transaction, picked, fundingSum: sum, change };
    if (params.feeSompi != null || !calcMass) break;
    // Mass is measured on the SIGNED shape, not the one the wallet is handed:
    // each funding input grows by a 66-byte Schnorr push once signed, and a fee
    // sized against the unsigned bytes would be short by exactly that much.
    const massShape = {
      ...transaction,
      inputs: inputs.map((inp, i) =>
        i >= firstFundingIdx ? { ...inp, signatureScript: `41${"00".repeat(65)}` } : inp,
      ),
    };
    const mass = BigInt(calcMass(network, massShape));
    const next = resolveNetworkFee({
      feeSompi: null,
      mass,
      feeRateSompiPerGram: params.feeRateSompiPerGram,
      minNetworkFeeSompi: params.minNetworkFeeSompi,
      maxNetworkFeeSompi: params.maxNetworkFeeSompi,
    });
    if (next.fee === feeInfo.fee) { feeInfo = next; break; }
    feeInfo = next;
    if (round === 5) throw new Error("curve: the fee and funding selection did not converge");
  }

  const { transaction, picked, fundingSum, change } = built;
  const fundingIndexes = picked.map((_, i) => firstFundingIdx + i);
  const signInputs = fundingIndexes.map((index) => ({ index, sighashType: 1 })); // SIGHASH_ALL

  const review = buildReviewRows({ isBuy, quote, transaction, change, feeInfo, tokenIn });

  const built0 = {
    kind,
    transaction,
    signInputs,
    fundingIndexes,
    outputs: review,
    quote,
    feeKeys,
    networkFeeSompi: feeInfo.fee,
    feeSource: feeInfo.source,
    massSompi: feeInfo.mass ?? null,
    changeSompi: change,
    fundingSum,
    raw: {
      state: st,
      inventory,
      tokenCells,
      curveArgs,
      inventoryArgs,
      tokenCellArgs,
      newStates,
      witnesses,
      userXOnly,
      changeScript,
      firstFundingIdx,
      tokenIn,
    },
  };
  built0.verify = () => verifyTrade(built0, rawState, params);
  return built0;
}

function kasStr(sompi) {
  const s = BigInt(sompi);
  const whole = s / 100_000_000n;
  const frac = (s % 100_000_000n).toString().padStart(8, "0").replace(/0+$/, "");
  return frac ? `${whole}.${frac} KAS` : `${whole} KAS`;
}

function buildReviewRows({ isBuy, quote, transaction, change, feeInfo, tokenIn }) {
  const o = transaction.outputs;
  const row = (i, label, kindName, detail) => ({
    label,
    kind: kindName,
    value_sompi: o[i].value.toString(),
    detail,
  });
  const rows = isBuy
    ? [
        row(0, "curve continuation", "covenant", `reserve → ${quote.newReserve}, holds ${kasStr(o[0].value)}`),
        row(1, "market inventory cell", "covenant", `${quote.newReserve} $KASCOV`),
        row(2, "your new $KASCOV cell", "you", `${quote.tokenOut} $KASCOV`),
        row(3, "creator fee (0.25%)", "fee", kasStr(o[3].value)),
        row(4, "platform fee (0.90%)", "fee", kasStr(o[4].value)),
        row(5, "dev fee (0.10%, min 0.2 KAS)", "fee", kasStr(o[5].value)),
        row(6, "change back to you", "you", kasStr(change)),
      ]
    : [
        row(0, "curve continuation", "covenant", `reserve → ${quote.newReserve}, holds ${kasStr(o[0].value)}`),
        row(1, "market inventory cell", "covenant", `${quote.newReserve} $KASCOV`),
        row(2, "creator fee (0.25%)", "fee", kasStr(o[2].value)),
        row(3, "platform fee (0.90%)", "fee", kasStr(o[3].value)),
        row(4, "dev fee (0.10%, min 0.2 KAS)", "fee", kasStr(o[4].value)),
        row(5, "proceeds + change back to you", "you", `${kasStr(change)} for ${tokenIn} $KASCOV`),
      ];
  rows.push({
    label: "network fee",
    kind: "network",
    value_sompi: feeInfo.fee.toString(),
    detail: `${kasStr(feeInfo.fee)} (${feeInfo.source}${feeInfo.mass != null ? `, mass ${feeInfo.mass}` : ""}) — not an output`,
  });
  return rows;
}

// ── verify() ──────────────────────────────────────────────────────────────────
//
// An independent re-derivation of the built transaction. It re-runs the quote
// from the curve state, recomputes every output value and script from first
// principles, re-reads the built signatureScripts BYTE BY BYTE (rather than
// trusting the values the builder started from), checks the value equation, and
// refuses any output paying a script the trade did not authorize.
//
// It returns { ok:false, reason, checks } on any mismatch and NEVER throws — the
// page blocks signing on ok:false, so a throw here would be a crash where a
// refusal is wanted.

function verifyTrade(built, rawState, params) {
  const checks = [];
  const add = (name, ok, detail) => { checks.push({ name, ok, detail: detail ?? "" }); return ok; };
  try {
    const isBuy = built.kind === "buy";
    const tx = built.transaction;
    const r = built.raw;

    // 1. the curve state, re-normalised from the caller's original object
    const st = normalizeCurveState(rawState);
    add("curve state accepted (supported build, one live cell, program hashes to the cell)", true,
      `cell ${st.outpoint.transactionId}:${st.outpoint.index}`);

    // 2. the quote, recomputed — from what the TRADER asked for and what the
    //    cells they handed over actually commit to, not from anything the
    //    builder wrote down along the way.
    const askedKasIn = isBuy ? BigInt(params.kasInSompi) : 0n;
    const askedTokenIn = isBuy
      ? 0n
      : (params.tokenCells || []).reduce((s, c) => s + parseCellState(c.programHex || c.program_hex).amount, 0n);
    const q = isBuy
      ? quoteBuy({ reserve: st.reserve, value: st.value }, askedKasIn)
      : quoteSell({ reserve: st.reserve, value: st.value }, askedTokenIn);
    if (!add("the transaction implements the trade the trader asked for",
      isBuy ? askedKasIn === built.quote.kasIn : askedTokenIn === r.tokenIn,
      isBuy ? `${askedKasIn} sompi in` : `${askedTokenIn} $KASCOV in`)) {
      return { ok: false, reason: "the built trade is not the trade that was requested", checks };
    }
    const qOk =
      q.newReserve === built.quote.newReserve &&
      q.curveValueAfter === built.quote.curveValueAfter &&
      (isBuy ? q.tokenOut === built.quote.tokenOut : q.kasOut === built.quote.kasOut);
    if (!add("constant-product quote reproduces", qOk,
      isBuy ? `tokenOut ${q.tokenOut}, reserve ${st.reserve} → ${q.newReserve}`
            : `kasOut ${q.kasOut}, reserve ${st.reserve} → ${q.newReserve}`)) {
      return { ok: false, reason: "the quote did not reproduce on re-derivation", checks };
    }

    // 3. the quantum
    const moved = isBuy ? askedKasIn : q.kasOut;
    if (!add("KAS moves in whole SCALE steps", moved % SCALE === 0n, `${moved} sompi`)) {
      return { ok: false, reason: "the KAS amount is not a whole multiple of SCALE", checks };
    }

    // 4. the shape, before anything indexes into it
    const expectedOutputCount = isBuy ? 7 : 6;
    if (!add("output count", tx.outputs.length === expectedOutputCount,
      `${tx.outputs.length} built, ${expectedOutputCount} expected`)) {
      return { ok: false, reason: "the transaction has the wrong number of outputs", checks };
    }
    const expectedInputCount = 2 + r.tokenCells.length + built.fundingIndexes.length;
    if (!add("input count", tx.inputs.length === expectedInputCount,
      `${tx.inputs.length} built, ${expectedInputCount} expected`)) {
      return { ok: false, reason: "the transaction has the wrong number of inputs", checks };
    }

    // 5. fee legs, recomputed from basis points
    const legs = feeLegs(moved);
    const feeKeys = feeKeysFromCurveProgram(st.programHex);
    const feeOffset = isBuy ? 3 : 2;
    const legOk =
      tx.outputs[feeOffset].value === legs.creator &&
      tx.outputs[feeOffset + 1].value === legs.platform &&
      tx.outputs[feeOffset + 2].value === legs.dev &&
      scriptHexOf(tx.outputs[feeOffset].scriptPublicKey) === p2pkScriptPubKey(feeKeys.creator) &&
      scriptHexOf(tx.outputs[feeOffset + 1].scriptPublicKey) === p2pkScriptPubKey(feeKeys.platform) &&
      scriptHexOf(tx.outputs[feeOffset + 2].scriptPublicKey) === p2pkScriptPubKey(feeKeys.dev);
    if (!add("three fee legs match the basis points and the keys baked in the program", legOk,
      `${legs.creator} / ${legs.platform} / ${legs.dev} sompi`)) {
      return { ok: false, reason: "a fee leg's value or recipient is wrong", checks };
    }

    // 6. every output value and script, rebuilt independently
    const b1 = q.newReserve;
    const expected = isBuy
      ? [
          [q.curveValueAfter, p2shScriptPubKey(nextCurveProgram(st.programHex, b1))],
          [CELL_DUST_SOMPI, kcc20CellScriptPubKey(r.inventory.programHex, st.marketCovenantId, 0x02, b1, 0)],
          [CELL_DUST_SOMPI, kcc20CellScriptPubKey(r.inventory.programHex, r.userXOnly, 0x03, q.tokenOut, 0)],
          [legs.creator, p2pkScriptPubKey(feeKeys.creator)],
          [legs.platform, p2pkScriptPubKey(feeKeys.platform)],
          [legs.dev, p2pkScriptPubKey(feeKeys.dev)],
          [built.changeSompi, p2pkScriptPubKey(r.userXOnly)],
        ]
      : [
          [q.curveValueAfter, p2shScriptPubKey(nextCurveProgram(st.programHex, b1))],
          [CELL_DUST_SOMPI, kcc20CellScriptPubKey(r.inventory.programHex, st.marketCovenantId, 0x02, b1, 0)],
          [legs.creator, p2pkScriptPubKey(feeKeys.creator)],
          [legs.platform, p2pkScriptPubKey(feeKeys.platform)],
          [legs.dev, p2pkScriptPubKey(feeKeys.dev)],
          [built.changeSompi, p2pkScriptPubKey(r.userXOnly)],
        ];
    for (let i = 0; i < expected.length; i++) {
      const okV = tx.outputs[i].value === expected[i][0];
      const okS = scriptHexOf(tx.outputs[i].scriptPublicKey) === expected[i][1];
      if (!add(`output ${i} value and script`, okV && okS,
        `${tx.outputs[i].value} → ${scriptHexOf(tx.outputs[i].scriptPublicKey)}`)) {
        return { ok: false, reason: `output ${i} does not match its independent re-derivation`, checks };
      }
    }

    // 7. nobody unauthorized is paid. Scripts, not addresses: an address is a
    //    rendering of a script, and it is the script that moves the money.
    const authorized = new Set(expected.map((e) => e[1]));
    const stray = tx.outputs.find((o) => !authorized.has(scriptHexOf(o.scriptPublicKey)));
    if (!add("no output pays a script the trade did not authorize", !stray,
      `${authorized.size} authorized scripts`)) {
      return { ok: false, reason: "an output pays an unauthorized script", checks };
    }

    // 8. change goes to the trader and nowhere else
    const changeScript = scriptHexOf(tx.outputs[tx.outputs.length - 1].scriptPublicKey);
    if (!add("change pays the trader's own P2PK key", changeScript === p2pkScriptPubKey(r.userXOnly),
      changeScript)) {
      return { ok: false, reason: "the change output does not pay the trader", checks };
    }

    // 9. the value equation, with the fee falling out of it
    const inSum = tx.inputs.reduce((s, i) => s + i.utxo.amount, 0n);
    const outSum = tx.outputs.reduce((s, o) => s + o.value, 0n);
    const fee = inSum - outSum;
    if (!add("inputs cover outputs", inSum >= outSum, `${inSum} in, ${outSum} out`)) {
      return { ok: false, reason: "the transaction spends more than it funds", checks };
    }
    if (!add("the implied network fee equals the fee the builder chose", fee === built.networkFeeSompi,
      `${fee} sompi = ${kasStr(fee)}`)) {
      return { ok: false, reason: "the value equation does not close on the stated fee", checks };
    }
    const cap = params.maxNetworkFeeSompi != null ? BigInt(params.maxNetworkFeeSompi) : NETWORK_FEE_CAP_SOMPI;
    if (!add("the network fee is inside the safety cap", fee > 0n && fee <= cap, `cap ${cap} sompi`)) {
      return { ok: false, reason: "the network fee is zero, negative or above the safety cap", checks };
    }

    // 10. the curve input: right outpoint, right pushes, and a reveal that
    //    hashes to the cell it spends.
    const cin = tx.inputs[0];
    const sameOutpoint =
      cin.previousOutpoint.transactionId === st.outpoint.transactionId &&
      cin.previousOutpoint.index === st.outpoint.index;
    if (!add("input 0 spends the live curve cell", sameOutpoint,
      `${cin.previousOutpoint.transactionId}:${cin.previousOutpoint.index}`)) {
      return { ok: false, reason: "input 0 is not the live curve cell", checks };
    }
    const cargs = parseScriptPushes(cin.signatureScript);
    if (!add("the curve input carries 12 pushes and no signature", cargs.length === 12,
      `${cargs.length} items`)) {
      return { ok: false, reason: "the curve input does not carry the proven 12-push shape", checks };
    }
    const revealHex = toHex(argToBytes(cargs[11]));
    if (!add("the curve reveal hashes to the cell being spent",
      p2shScriptPubKey(revealHex) === st.scriptPublicKey, st.scriptPublicKey)) {
      return { ok: false, reason: "the revealed curve program is not the one the cell commits to", checks };
    }
    const argMoved = isBuy ? argToBigInt(cargs[0]) : argToBigInt(cargs[1]);
    const argReserve = argToBigInt(cargs[4]);
    const argOwner = toHex(argToBytes(cargs[6]));
    const argSelector = argToBigInt(cargs[10]);
    const argsOk =
      argMoved === moved &&
      argReserve === b1 &&
      argOwner === r.userXOnly &&
      argSelector === (isBuy ? 1n : 2n) &&
      (isBuy ? argToBigInt(cargs[1]) === q.tokenOut && argToBigInt(cargs[8]) === q.tokenOut
             : argToBigInt(cargs[0]) === askedTokenIn) &&
      toHex(argToBytes(cargs[2])) === st.marketCovenantId;
    if (!add("the curve args re-read from the built bytes match the quote", argsOk,
      `selector ${argSelector}, moved ${argMoved}, reserve → ${argReserve}`)) {
      return { ok: false, reason: "the curve input's own bytes disagree with the quote", checks };
    }

    // 11. every kcc20 input: identical transfer args, its own reveal, and a
    //     witness pointing at an input that really can authorize it.
    const kcc20Indexes = [1, ...r.tokenCells.map((_, i) => 2 + i)];
    const expectOwners = r.newStates.map((s) => s.owner).join("");
    const expectTypes = r.newStates.map((s) => s.identifierType.toString(16).padStart(2, "0")).join("");
    for (const idx of kcc20Indexes) {
      const inp = tx.inputs[idx];
      const ps = parseScriptPushes(inp.signatureScript);
      if (!add(`kcc20 input ${idx} carries 7 pushes and an empty sigs array`,
        ps.length === 7 && argToBytes(ps[4]).length === 0, `${ps.length} items`)) {
        return { ok: false, reason: `kcc20 input ${idx} is not the proven transfer shape`, checks };
      }
      const owners = toHex(argToBytes(ps[0]));
      const types = toHex(argToBytes(ps[1]));
      const wits = Array.from(argToBytes(ps[5]));
      const cellReveal = toHex(argToBytes(ps[6]));
      const shapeOk =
        owners === expectOwners &&
        types === expectTypes &&
        wits.length === kcc20Indexes.length &&
        wits.every((w, k) => w === r.witnesses[k]) &&
        p2shScriptPubKey(cellReveal) === scriptHexOf(inp.utxo.scriptPublicKey);
      if (!add(`kcc20 input ${idx} states, witnesses and reveal`, shapeOk,
        `witnesses [${wits.join(",")}], reveal hashes to ${p2shScriptPubKey(cellReveal)}`)) {
        return { ok: false, reason: `kcc20 input ${idx} does not verify against its own bytes`, checks };
      }
    }
    // each witness must name an input that can actually authorize its cell
    for (let k = 0; k < kcc20Indexes.length; k++) {
      const cellIdx = kcc20Indexes[k];
      const cellState = parseCellState(toHex(argToBytes(parseScriptPushes(tx.inputs[cellIdx].signatureScript)[6])));
      const w = r.witnesses[k];
      let wOk = false;
      if (cellState.identifierType === 0x02) {
        // covenant-id owned: the curve input, whose covenant id is the owner
        wOk = w === 0 && cellState.owner === st.marketCovenantId;
      } else if (cellState.identifierType === 0x03) {
        // presence owned: a P2PK input carrying that key
        wOk = w >= r.firstFundingIdx &&
          w < tx.inputs.length &&
          scriptHexOf(tx.inputs[w].utxo.scriptPublicKey) === p2pkScriptPubKey(cellState.owner);
      }
      if (!add(`kcc20 input ${cellIdx} witness ${w} can authorize a type-0x${cellState.identifierType.toString(16).padStart(2, "0")} cell`, wOk)) {
        return { ok: false, reason: `kcc20 input ${cellIdx} points at an input that cannot authorize it`, checks };
      }
    }

    // 12. exactly the funding inputs are offered for signature, and every
    //     covenant input goes out with its script already final.
    const fundingSet = new Set(built.fundingIndexes);
    let sigOk = built.signInputs.length === built.fundingIndexes.length &&
      built.signInputs.every((s) => fundingSet.has(s.index) && s.sighashType === 1);
    for (let i = 0; i < tx.inputs.length && sigOk; i++) {
      const isFunding = fundingSet.has(i);
      const sig = tx.inputs[i].signatureScript || "";
      if (isFunding) {
        sigOk = sig === "" && scriptHexOf(tx.inputs[i].utxo.scriptPublicKey) === p2pkScriptPubKey(r.userXOnly);
      } else {
        sigOk = sig.length > 0 && i < r.firstFundingIdx;
      }
    }
    if (!add("only the trader's own funding inputs are offered for signature", sigOk,
      `signing [${built.fundingIndexes.join(",")}] of ${tx.inputs.length} inputs`)) {
      return { ok: false, reason: "the signing plan does not match the funding inputs", checks };
    }

    // 13. the covenant bindings the outputs carry (they are part of the
    //     transaction id, so a wrong one is a different transaction).
    const bindOk =
      tx.outputs[0].covenant &&
      tx.outputs[0].covenant.authorizingInput === 0 &&
      tx.outputs[0].covenant.covenantId === st.marketCovenantId &&
      tx.outputs[1].covenant &&
      tx.outputs[1].covenant.authorizingInput === 1 &&
      tx.outputs[1].covenant.covenantId === st.tokenCovid &&
      (!isBuy || (tx.outputs[2].covenant && tx.outputs[2].covenant.authorizingInput === 1 &&
        tx.outputs[2].covenant.covenantId === st.tokenCovid)) &&
      tx.outputs.slice(isBuy ? 3 : 2).every((o) => !o.covenant);
    if (!add("output covenant bindings", Boolean(bindOk), `token ${st.tokenCovid}`)) {
      return { ok: false, reason: "an output's covenant binding is wrong or missing", checks };
    }

    return { ok: true, reason: null, checks };
  } catch (e) {
    checks.push({ name: "verify raised", ok: false, detail: e && e.message ? e.message : String(e) });
    return { ok: false, reason: `verification could not complete: ${e && e.message ? e.message : e}`, checks };
  }
}
