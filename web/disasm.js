/* kascov — post-Toccata Kaspa Script disassembler.
   A faithful port of crates/kascov-decode/src/disasm.rs; the opcode table
   comes from rusty-kaspa crypto/txscript at the workspace's pinned rev.
   Exposed as window.kascovDisasm for app.js (no build step, no modules). */
(() => {
'use strict';

/* Op2..Op16 (0x52..0x60) and OpData (0x01..0x4b) are handled as ranges in
   opcodeInfo; everything else is listed here as [name, group]. Absent
   opcodes (0x4a? no — 0xca, 0xdb..0xf9, 0xfc) are unknown. */
const OPS = {
  0x00: ['OpFalse', 'push'],
  0x4c: ['OpPushData1', 'push'],
  0x4d: ['OpPushData2', 'push'],
  0x4e: ['OpPushData4', 'push'],
  0x4f: ['Op1Negate', 'push'],
  0x50: ['OpReserved', 'standard'],
  0x51: ['OpTrue', 'push'],
  0x61: ['OpNop', 'standard'],
  0x62: ['OpVer', 'standard'],
  0x63: ['OpIf', 'standard'],
  0x64: ['OpNotIf', 'standard'],
  0x65: ['OpVerIf', 'standard'],
  0x66: ['OpVerNotIf', 'standard'],
  0x67: ['OpElse', 'standard'],
  0x68: ['OpEndIf', 'standard'],
  0x69: ['OpVerify', 'standard'],
  0x6a: ['OpReturn', 'standard'],
  0x6b: ['OpToAltStack', 'standard'],
  0x6c: ['OpFromAltStack', 'standard'],
  0x6d: ['Op2Drop', 'standard'],
  0x6e: ['Op2Dup', 'standard'],
  0x6f: ['Op3Dup', 'standard'],
  0x70: ['Op2Over', 'standard'],
  0x71: ['Op2Rot', 'standard'],
  0x72: ['Op2Swap', 'standard'],
  0x73: ['OpIfDup', 'standard'],
  0x74: ['OpDepth', 'standard'],
  0x75: ['OpDrop', 'standard'],
  0x76: ['OpDup', 'standard'],
  0x77: ['OpNip', 'standard'],
  0x78: ['OpOver', 'standard'],
  0x79: ['OpPick', 'standard'],
  0x7a: ['OpRoll', 'standard'],
  0x7b: ['OpRot', 'standard'],
  0x7c: ['OpSwap', 'standard'],
  0x7d: ['OpTuck', 'standard'],
  0x7e: ['OpCat', 'standard'],
  0x7f: ['OpSubstr', 'standard'],
  0x80: ['OpLeft', 'standard'],
  0x81: ['OpRight', 'standard'],
  0x82: ['OpSize', 'standard'],
  0x83: ['OpInvert', 'standard'],
  0x84: ['OpAnd', 'standard'],
  0x85: ['OpOr', 'standard'],
  0x86: ['OpXor', 'standard'],
  0x87: ['OpEqual', 'standard'],
  0x88: ['OpEqualVerify', 'standard'],
  0x89: ['OpReserved1', 'standard'],
  0x8a: ['OpReserved2', 'standard'],
  0x8b: ['Op1Add', 'standard'],
  0x8c: ['Op1Sub', 'standard'],
  0x8d: ['Op2Mul', 'standard'],
  0x8e: ['Op2Div', 'standard'],
  0x8f: ['OpNegate', 'standard'],
  0x90: ['OpAbs', 'standard'],
  0x91: ['OpNot', 'standard'],
  0x92: ['Op0NotEqual', 'standard'],
  0x93: ['OpAdd', 'standard'],
  0x94: ['OpSub', 'standard'],
  0x95: ['OpMul', 'standard'],
  0x96: ['OpDiv', 'standard'],
  0x97: ['OpMod', 'standard'],
  0x98: ['OpLShift', 'standard'],
  0x99: ['OpRShift', 'standard'],
  0x9a: ['OpBoolAnd', 'standard'],
  0x9b: ['OpBoolOr', 'standard'],
  0x9c: ['OpNumEqual', 'standard'],
  0x9d: ['OpNumEqualVerify', 'standard'],
  0x9e: ['OpNumNotEqual', 'standard'],
  0x9f: ['OpLessThan', 'standard'],
  0xa0: ['OpGreaterThan', 'standard'],
  0xa1: ['OpLessThanOrEqual', 'standard'],
  0xa2: ['OpGreaterThanOrEqual', 'standard'],
  0xa3: ['OpMin', 'standard'],
  0xa4: ['OpMax', 'standard'],
  0xa5: ['OpWithin', 'standard'],
  0xa6: ['OpZkPrecompile', 'zk'],
  0xa7: ['OpBlake2bWithKey', 'standard'],
  0xa8: ['OpSHA256', 'standard'],
  0xa9: ['OpCheckMultiSigECDSA', 'standard'],
  0xaa: ['OpBlake2b', 'standard'],
  0xab: ['OpCheckSigECDSA', 'standard'],
  0xac: ['OpCheckSig', 'standard'],
  0xad: ['OpCheckSigVerify', 'standard'],
  0xae: ['OpCheckMultiSig', 'standard'],
  0xaf: ['OpCheckMultiSigVerify', 'standard'],
  0xb0: ['OpCheckLockTimeVerify', 'standard'],
  0xb1: ['OpCheckSequenceVerify', 'standard'],
  0xb2: ['OpTxVersion', 'introspection'],
  0xb3: ['OpTxInputCount', 'introspection'],
  0xb4: ['OpTxOutputCount', 'introspection'],
  0xb5: ['OpTxLockTime', 'introspection'],
  0xb6: ['OpTxSubnetId', 'introspection'],
  0xb7: ['OpTxGas', 'introspection'],
  0xb8: ['OpTxPayloadSubstr', 'introspection'],
  0xb9: ['OpTxInputIndex', 'introspection'],
  0xba: ['OpOutpointTxId', 'introspection'],
  0xbb: ['OpOutpointIndex', 'introspection'],
  0xbc: ['OpTxInputScriptSigSubstr', 'introspection'],
  0xbd: ['OpTxInputSeq', 'introspection'],
  0xbe: ['OpTxInputAmount', 'introspection'],
  0xbf: ['OpTxInputSpk', 'introspection'],
  0xc0: ['OpTxInputDaaScore', 'introspection'],
  0xc1: ['OpTxInputIsCoinbase', 'introspection'],
  0xc2: ['OpTxOutputAmount', 'introspection'],
  0xc3: ['OpTxOutputSpk', 'introspection'],
  0xc4: ['OpTxPayloadLen', 'introspection'],
  0xc5: ['OpTxInputSpkLen', 'introspection'],
  0xc6: ['OpTxInputSpkSubstr', 'introspection'],
  0xc7: ['OpTxOutputSpkLen', 'introspection'],
  0xc8: ['OpTxOutputSpkSubstr', 'introspection'],
  0xc9: ['OpTxInputScriptSigLen', 'introspection'],
  0xcb: ['OpAuthOutputCount', 'covenant'],
  0xcc: ['OpAuthOutputIdx', 'covenant'],
  0xcd: ['OpNum2Bin', 'introspection'],
  0xce: ['OpBin2Num', 'introspection'],
  0xcf: ['OpInputCovenantId', 'covenant'],
  0xd0: ['OpCovInputCount', 'covenant'],
  0xd1: ['OpCovInputIdx', 'covenant'],
  0xd2: ['OpCovOutputCount', 'covenant'],
  0xd3: ['OpCovOutputIdx', 'covenant'],
  0xd4: ['OpChainblockSeqCommit', 'covenant'],
  0xd5: ['OpOutputCovenantId', 'covenant'],
  0xd6: ['OpOutputAuthorizingInput', 'covenant'],
  0xd7: ['OpCheckSigFromStack', 'standard'],
  0xd8: ['OpCheckSigFromStackECDSA', 'standard'],
  0xd9: ['OpBlake3', 'standard'],
  0xda: ['OpBlake3WithKey', 'standard'],
  0xfa: ['OpSmallInteger', 'standard'],
  0xfb: ['OpPubKeys', 'standard'],
  0xfd: ['OpPubKeyHash', 'standard'],
  0xfe: ['OpPubKey', 'standard'],
  0xff: ['OpInvalidOpCode', 'standard'],
};

function opcodeInfo(opcode) {
  if (opcode >= 0x01 && opcode <= 0x4b) return ['OpData', 'push'];
  if (opcode >= 0x52 && opcode <= 0x60) return ['Op' + (opcode - 0x50), 'push'];
  return OPS[opcode] || ['OpUnknown', 'unknown'];
}

/* Disassemble a script. Mirrors the Rust semantics exactly, including the
   quirk that a push whose LENGTH bytes are missing becomes a data-less
   instruction (not truncated) while a push whose DATA runs past the end
   flags truncation and stops. */
function disassemble(bytes) {
  const out = [];
  let i = 0;
  while (i < bytes.length) {
    const offset = i;
    const opcode = bytes[i];
    i += 1;
    const [name, group] = opcodeInfo(opcode);

    let dataLen = null;
    if (opcode >= 0x01 && opcode <= 0x4b) {
      dataLen = opcode;
    } else if (opcode === 0x4c) {
      if (i < bytes.length) { dataLen = bytes[i]; i += 1; }
    } else if (opcode === 0x4d) {
      if (i + 2 <= bytes.length) { dataLen = bytes[i] | (bytes[i + 1] << 8); i += 2; }
    } else if (opcode === 0x4e) {
      if (i + 4 <= bytes.length) {
        dataLen = (bytes[i] | (bytes[i + 1] << 8) | (bytes[i + 2] << 16) | (bytes[i + 3] << 24)) >>> 0;
        i += 4;
      }
    }

    if (dataLen === null) {
      out.push({ offset, opcode, name, group, data: null });
    } else if (i + dataLen > bytes.length) {
      out.push({ offset, opcode, name, group, data: null });
      return { instructions: out, truncated: true };
    } else {
      out.push({ offset, opcode, name, group, data: bytes.slice(i, i + dataLen) });
      i += dataLen;
    }
  }
  return { instructions: out, truncated: false };
}

function toHex(bytes) {
  let s = '';
  for (const b of bytes) s += b.toString(16).padStart(2, '0');
  return s;
}

/* 'name 0xdata' for non-empty pushes, matching the Rust Display impl. */
function toAsm(inst) {
  return inst.data && inst.data.length ? `${inst.name} 0x${toHex(inst.data)}` : inst.name;
}

/* Forgiving hex reader: whitespace, newlines, an optional 0x prefix.
   Returns Uint8Array, or null when the input isn't clean hex. */
function parseHex(text) {
  let s = String(text).replace(/\s+/g, '').toLowerCase();
  if (s.startsWith('0x')) s = s.slice(2);
  if (s.length === 0 || s.length % 2 !== 0 || /[^0-9a-f]/.test(s)) return null;
  const out = new Uint8Array(s.length / 2);
  for (let k = 0; k < out.length; k++) out[k] = parseInt(s.slice(k * 2, k * 2 + 2), 16);
  return out;
}


/* SilverScript example contracts (kaspanet/silverscript), each compiled
   twice with sentinel constructor args — skeletons derive at load. Kept
   byte-identical to kascov-decode's embedded dumps. */
const SS_DUMPS = [
  {
    name: 'SilverScript · Mecenas',
    a: '6b6c76009c637502e803b100c3201111111111111111111111111111111111111111111111111111111111111111030000207c7e01ac7e876902e803b9be760400e1f50594527994760400e1f505547993a16300c252795479949c696700c20400e1f5059c6951c3b9bf876951c2789c6968007a75007a75007a75516776519c637578aa2033333333333333333333333333333333333333333333333333333333333333338769765279ac69757551677500696868',
    b: '6b6c76009c637502d007b100c3202222222222222222222222222222222222222222222222222222222222222222030000207c7e01ac7e876902e803b9be760480b2e60e94527994760480b2e60e547993a16300c252795479949c696700c20480b2e60e9c6951c3b9bf876951c2789c6968007a75007a75007a75516776519c637578aa2044444444444444444444444444444444444444444444444444444444444444448769765279ac69757551677500696868',
    sentinels: [
      ['recipient', '11'.repeat(32)],
      ['funder_hash', '33'.repeat(32)],
      ['pledge', '00e1f505'],
      ['period', 'e803'],
    ],
    /* generator metadata: how each constructor arg is entered and encoded */
    params: [
      { name: 'recipient', kind: 'pubkey', source: 'recipient', hint: 'x-only public key that can claim each pledge' },
      { name: 'funder_hash', kind: 'hash32', source: 'funder', hint: 'blake2b-256 of the funder — set to YOUR keygen blake2b to reclaim the coin yourself' },
      { name: 'pledge', kind: 'amount', source: 'pledge', hint: 'how much the recipient may take per period' },
      { name: 'period', kind: 'daa', source: 'period', hint: 'claim interval in DAA ticks (≈10 per second)' },
    ],
  },
  {
    name: 'SilverScript · Escrow',
    a: '78aa2033333333333333333333333333333333333333333333333333333333333333338769765279ac6900c2b9be02e803949c6900c3201111111111111111111111111111111111111111111111111111111111111111030000207c7e01ac7e8700c3202222222222222222222222222222222222222222222222222222222222222222030000207c7e01ac7e879b69757551',
    b: '78aa2044444444444444444444444444444444444444444444444444444444444444448769765279ac6900c2b9be02e803949c6900c3202222222222222222222222222222222222222222222222222222222222222222030000207c7e01ac7e8700c3201111111111111111111111111111111111111111111111111111111111111111030000207c7e01ac7e879b69757551',
    sentinels: [
      ['arbiter_hash', '33'.repeat(32)],
      ['buyer', '11'.repeat(32)],
      ['seller', '22'.repeat(32)],
    ],
    params: [
      { name: 'arbiter_hash', kind: 'hash32', source: 'arbiter', hint: 'blake2b-256 of the arbiter’s public key' },
      { name: 'buyer', kind: 'pubkey', source: 'buyer', hint: 'the buyer’s x-only public key' },
      { name: 'seller', kind: 'pubkey', source: 'seller', hint: 'the seller’s x-only public key' },
    ],
  },
  {
    name: 'SilverScript · LastWill',
    a: '6b6c76009c637502b400b178aa2033333333333333333333333333333333333333333333333333333333333333338769765279ac697575516776519c637578aa2044444444444444444444444444444444444444444444444444444444444444448769765279ac697575516776529c637578aa2011111111111111111111111111111111111111111111111111111111111111118769765279ac6900c2b9be02e803949c6900c3b9bf876975755167750069686868',
    b: '6b6c76009c637502b400b178aa2044444444444444444444444444444444444444444444444444444444444444448769765279ac697575516776519c637578aa2033333333333333333333333333333333333333333333333333333333333333338769765279ac697575516776529c637578aa2022222222222222222222222222222222222222222222222222222222222222228769765279ac6900c2b9be02e803949c6900c3b9bf876975755167750069686868',
    sentinels: [
      ['inheritor_hash', '33'.repeat(32)],
      ['cold_hash', '44'.repeat(32)],
      ['hot_hash', '11'.repeat(32)],
    ],
    params: [
      { name: 'inheritor_hash', kind: 'hash32', source: 'inheritor', hint: 'blake2b-256 of the inheritor’s public key' },
      { name: 'cold_hash', kind: 'hash32', source: 'cold', hint: 'blake2b-256 of the cold key — set to YOUR keygen blake2b to spend it back yourself' },
      { name: 'hot_hash', kind: 'hash32', source: 'hot', hint: 'blake2b-256 of the everyday (hot) public key' },
    ],
  },
];

/* ---- skeleton template matching (port of kascov-decode's Skeleton) ---- */

function pushValue(inst) {
  if (inst.data && inst.data.length >= 0) return Array.from(inst.data);
  if (inst.opcode === 0x00) return [];
  if (inst.opcode === 0x4f) return [0x81];
  if (inst.opcode >= 0x51 && inst.opcode <= 0x60) return [inst.opcode - 0x50];
  return null;
}

const sameBytes = (a, b) => a.length === b.length && a.every((v, i) => v === b[i]);

/* ---- emit: rebuild a compiled contract with new constructor args ---- */

/* Kaspa script-number encoding (mirror of kascov-decode's snum): minimal
   little-endian sign-magnitude; a sign-guard 0x00 is appended when the top
   byte's high bit is set. 180 → b4 00 · 1000 → e8 03 · 1e8 → 00 e1 f5 05 */
function snumEncode(v) {
  let n = typeof v === 'bigint' ? v : BigInt(v);
  if (n === 0n) return [];
  const neg = n < 0n;
  if (neg) n = -n;
  const out = [];
  while (n > 0n) { out.push(Number(n & 0xffn)); n >>= 8n; }
  if (out[out.length - 1] & 0x80) out.push(neg ? 0x80 : 0x00);
  else if (neg) out[out.length - 1] |= 0x80;
  return out;
}

function snumDecode(bytes) {
  if (!bytes.length) return 0n;
  let n = 0n;
  for (let i = bytes.length - 1; i >= 0; i--) {
    n = (n << 8n) | BigInt(i === bytes.length - 1 ? bytes[i] & 0x7f : bytes[i]);
  }
  return bytes[bytes.length - 1] & 0x80 ? -n : n;
}

/* Canonical push encoding — exactly what the SilverScript compiler's
   ScriptBuilder emits for a pushed value. */
function encodePush(value) {
  if (value.length === 0) return [0x00];
  if (value.length === 1 && value[0] === 0x81) return [0x4f];
  if (value.length === 1 && value[0] >= 1 && value[0] <= 16) return [0x50 + value[0]];
  if (value.length <= 75) return [value.length, ...value];
  if (value.length <= 0xff) return [0x4c, value.length, ...value];
  if (value.length <= 0xffff) return [0x4d, value.length & 0xff, value.length >> 8, ...value];
  return [0x4e, value.length & 0xff, (value.length >> 8) & 0xff, (value.length >> 16) & 0xff, (value.length >>> 24), ...value];
}

/* Walk a skeleton, keeping fixed ops/consts byte-identical to dump A and
   re-encoding only the slots. Returns Uint8Array or null on missing args. */
function emitSkeleton(skel, args) {
  const out = [];
  for (const item of skel.items) {
    if (item.slot !== undefined) {
      const v = args[item.slot];
      if (!v) return null;
      out.push(...encodePush(Array.from(v)));
    } else {
      out.push(...item.raw);
    }
  }
  return Uint8Array.from(out);
}

function emitFromSkeleton(name, args) {
  const skel = skeletons().find((s) => s.name === name);
  return skel ? emitSkeleton(skel, args) : null;
}

function deriveSkeleton(name, aHex, bHex, sentinels) {
  const aBytes = parseHex(aHex);
  const ia = disassemble(aBytes);
  const ib = disassemble(parseHex(bHex));
  if (ia.truncated || ib.truncated) return null;
  const A = ia.instructions, B = ib.instructions;
  if (A.length !== B.length) return null;
  const items = [];
  for (let i = 0; i < A.length; i++) {
    const x = A[i], y = B[i];
    /* fixed items keep their raw bytes from dump A so emit is byte-perfect
       even where the compiler chose a non-minimal push for a constant */
    const end = i + 1 < A.length ? A[i + 1].offset : aBytes.length;
    const raw = Array.from(aBytes.slice(x.offset, end));
    const px = x.group === 'push', py = y.group === 'push';
    if (!px && !py) {
      if (x.opcode !== y.opcode) return null;
      items.push({ op: x.opcode, raw });
    } else if (px && py) {
      const vx = pushValue(x), vy = pushValue(y);
      if (!vx || !vy) return null;
      if (sameBytes(vx, vy)) items.push({ const: vx, raw });
      else {
        const hit = sentinels.find(([, s]) => sameBytes(vx, Array.from(parseHex(s) || [])));
        if (!hit) return null;
        items.push({ slot: hit[0] });
      }
    } else return null;
  }
  return { name, items, order: sentinels.map(([l]) => l) };
}

let SS_SKELETONS = null;
function skeletons() {
  if (!SS_SKELETONS) {
    SS_SKELETONS = SS_DUMPS
      .map((d) => {
        const skel = deriveSkeleton(d.name, d.a, d.b, d.sentinels);
        if (!skel) return null;
        /* self-check: emitting with the sentinel args must reproduce dump A
           byte-for-byte — the generator only offers itself when this holds */
        const args = {};
        for (const [label, hex] of d.sentinels) args[label] = Array.from(parseHex(hex));
        const emitted = emitSkeleton(skel, args);
        skel.emitVerified = !!emitted && toHex(emitted) === d.a;
        skel.params = d.params || [];
        return skel;
      })
      .filter(Boolean);
  }
  return SS_SKELETONS;
}

/* Name a disassembled script if it matches a known contract or state shape;
   returns { name, fields: [{name, value(hex)}] } or null. */
function matchTemplates(instructions, bytes) {
  /* the ubiquitous state shapes first */
  if (bytes && (bytes.length === 34 || bytes.length === 35) &&
      bytes[0] === bytes.length - 2 && bytes[bytes.length - 1] === 0xac) {
    return { name: 'p2pk state', fields: [{ name: 'owner_pubkey', value: toHex(bytes.slice(1, -1)) }] };
  }
  if (bytes && bytes.length === 35 && bytes[0] === 0xaa && bytes[1] === 0x20 && bytes[34] === 0x87) {
    return { name: 'p2sh commitment', fields: [{ name: 'program_hash', value: toHex(bytes.slice(2, 34)) }] };
  }
  for (const skel of skeletons()) {
    if (instructions.length !== skel.items.length) continue;
    const values = new Map();
    let ok = true;
    for (let i = 0; i < skel.items.length && ok; i++) {
      const item = skel.items[i], inst = instructions[i];
      if (item.op !== undefined) {
        ok = inst.group !== 'push' && inst.opcode === item.op;
      } else if (item.const) {
        const v = pushValue(inst);
        ok = !!v && sameBytes(v, item.const);
      } else {
        const v = pushValue(inst);
        if (!v) { ok = false; break; }
        const prev = values.get(item.slot);
        if (prev && !sameBytes(prev, v)) { ok = false; break; }
        values.set(item.slot, v);
      }
    }
    if (ok) {
      return {
        name: skel.name,
        fields: skel.order
          .filter((l) => values.has(l))
          .map((l) => ({ name: l, value: toHex(Uint8Array.from(values.get(l))) })),
      };
    }
  }
  return null;
}

window.kascovDisasm = {
  disassemble, opcodeInfo, parseHex, toAsm, toHex, matchTemplates, SS_DUMPS,
  emitFromSkeleton, snumEncode, snumDecode, skeletonInfo: (name) => {
    const s = skeletons().find((x) => x.name === name);
    return s ? { params: s.params, emitVerified: s.emitVerified } : null;
  },
};

})();
