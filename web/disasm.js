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

window.kascovDisasm = { disassemble, opcodeInfo, parseHex, toAsm, toHex };

})();
