//! Kaspa Script disassembler, covering the post-Toccata opcode set
//! (KIP-17 introspection + covenant + ZK opcodes included).
//!
//! Opcode table extracted from rusty-kaspa `crypto/txscript/src/opcodes/mod.rs`
//! at rev 98a4ccd (the workspace's pinned dependency rev).

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OpGroup {
    /// Data pushes
    Push,
    /// Pre-Toccata standard opcodes
    Standard,
    /// KIP-17 transaction introspection (0xb2–0xc9, 0xcd, 0xce)
    Introspection,
    /// KIP-20 covenant opcodes (0xcb, 0xcc, 0xcf–0xd6)
    Covenant,
    /// KIP-16 ZK verification (OpZkPrecompile)
    Zk,
    Unknown,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Instruction {
    pub offset: usize,
    pub opcode: u8,
    pub name: &'static str,
    pub group: OpGroup,
    /// Pushed data, for push instructions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Vec<u8>>,
}

impl fmt::Display for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.data {
            Some(data) if !data.is_empty() => write!(f, "{} 0x{}", self.name, hex::encode(data)),
            _ => f.write_str(self.name),
        }
    }
}

/// Disassemble a script. Returns instructions plus a flag for truncated /
/// malformed tails (a push running past the end).
pub fn disassemble(script: &[u8]) -> (Vec<Instruction>, bool) {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < script.len() {
        let offset = i;
        let opcode = script[i];
        i += 1;
        let (name, group) = opcode_info(opcode);

        let data_len = match opcode {
            0x01..=0x4b => Some(opcode as usize),
            0x4c => script.get(i).map(|&n| {
                i += 1;
                n as usize
            }),
            0x4d => script.get(i..i + 2).map(|b| {
                i += 2;
                u16::from_le_bytes([b[0], b[1]]) as usize
            }),
            0x4e => script.get(i..i + 4).map(|b| {
                i += 4;
                u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize
            }),
            _ => None,
        };

        match data_len {
            None => out.push(Instruction { offset, opcode, name, group, data: None }),
            Some(len) => {
                let Some(data) = script.get(i..i + len) else {
                    out.push(Instruction { offset, opcode, name, group, data: None });
                    return (out, true);
                };
                i += len;
                out.push(Instruction { offset, opcode, name, group, data: Some(data.to_vec()) });
            }
        }
    }
    (out, false)
}

pub fn opcode_info(opcode: u8) -> (&'static str, OpGroup) {
    use OpGroup::*;
    match opcode {
        0x00 => ("OpFalse", Push),
        0x01..=0x4b => ("OpData", Push),
        0x4c => ("OpPushData1", Push),
        0x4d => ("OpPushData2", Push),
        0x4e => ("OpPushData4", Push),
        0x4f => ("Op1Negate", Push),
        0x50 => ("OpReserved", Standard),
        0x51 => ("OpTrue", Push),
        0x52 => ("Op2", Push),
        0x53 => ("Op3", Push),
        0x54 => ("Op4", Push),
        0x55 => ("Op5", Push),
        0x56 => ("Op6", Push),
        0x57 => ("Op7", Push),
        0x58 => ("Op8", Push),
        0x59 => ("Op9", Push),
        0x5a => ("Op10", Push),
        0x5b => ("Op11", Push),
        0x5c => ("Op12", Push),
        0x5d => ("Op13", Push),
        0x5e => ("Op14", Push),
        0x5f => ("Op15", Push),
        0x60 => ("Op16", Push),
        0x61 => ("OpNop", Standard),
        0x62 => ("OpVer", Standard),
        0x63 => ("OpIf", Standard),
        0x64 => ("OpNotIf", Standard),
        0x65 => ("OpVerIf", Standard),
        0x66 => ("OpVerNotIf", Standard),
        0x67 => ("OpElse", Standard),
        0x68 => ("OpEndIf", Standard),
        0x69 => ("OpVerify", Standard),
        0x6a => ("OpReturn", Standard),
        0x6b => ("OpToAltStack", Standard),
        0x6c => ("OpFromAltStack", Standard),
        0x6d => ("Op2Drop", Standard),
        0x6e => ("Op2Dup", Standard),
        0x6f => ("Op3Dup", Standard),
        0x70 => ("Op2Over", Standard),
        0x71 => ("Op2Rot", Standard),
        0x72 => ("Op2Swap", Standard),
        0x73 => ("OpIfDup", Standard),
        0x74 => ("OpDepth", Standard),
        0x75 => ("OpDrop", Standard),
        0x76 => ("OpDup", Standard),
        0x77 => ("OpNip", Standard),
        0x78 => ("OpOver", Standard),
        0x79 => ("OpPick", Standard),
        0x7a => ("OpRoll", Standard),
        0x7b => ("OpRot", Standard),
        0x7c => ("OpSwap", Standard),
        0x7d => ("OpTuck", Standard),
        0x7e => ("OpCat", Standard),
        0x7f => ("OpSubstr", Standard),
        0x80 => ("OpLeft", Standard),
        0x81 => ("OpRight", Standard),
        0x82 => ("OpSize", Standard),
        0x83 => ("OpInvert", Standard),
        0x84 => ("OpAnd", Standard),
        0x85 => ("OpOr", Standard),
        0x86 => ("OpXor", Standard),
        0x87 => ("OpEqual", Standard),
        0x88 => ("OpEqualVerify", Standard),
        0x89 => ("OpReserved1", Standard),
        0x8a => ("OpReserved2", Standard),
        0x8b => ("Op1Add", Standard),
        0x8c => ("Op1Sub", Standard),
        0x8d => ("Op2Mul", Standard),
        0x8e => ("Op2Div", Standard),
        0x8f => ("OpNegate", Standard),
        0x90 => ("OpAbs", Standard),
        0x91 => ("OpNot", Standard),
        0x92 => ("Op0NotEqual", Standard),
        0x93 => ("OpAdd", Standard),
        0x94 => ("OpSub", Standard),
        0x95 => ("OpMul", Standard),
        0x96 => ("OpDiv", Standard),
        0x97 => ("OpMod", Standard),
        0x98 => ("OpLShift", Standard),
        0x99 => ("OpRShift", Standard),
        0x9a => ("OpBoolAnd", Standard),
        0x9b => ("OpBoolOr", Standard),
        0x9c => ("OpNumEqual", Standard),
        0x9d => ("OpNumEqualVerify", Standard),
        0x9e => ("OpNumNotEqual", Standard),
        0x9f => ("OpLessThan", Standard),
        0xa0 => ("OpGreaterThan", Standard),
        0xa1 => ("OpLessThanOrEqual", Standard),
        0xa2 => ("OpGreaterThanOrEqual", Standard),
        0xa3 => ("OpMin", Standard),
        0xa4 => ("OpMax", Standard),
        0xa5 => ("OpWithin", Standard),
        0xa6 => ("OpZkPrecompile", Zk),
        0xa7 => ("OpBlake2bWithKey", Standard),
        0xa8 => ("OpSHA256", Standard),
        0xa9 => ("OpCheckMultiSigECDSA", Standard),
        0xaa => ("OpBlake2b", Standard),
        0xab => ("OpCheckSigECDSA", Standard),
        0xac => ("OpCheckSig", Standard),
        0xad => ("OpCheckSigVerify", Standard),
        0xae => ("OpCheckMultiSig", Standard),
        0xaf => ("OpCheckMultiSigVerify", Standard),
        0xb0 => ("OpCheckLockTimeVerify", Standard),
        0xb1 => ("OpCheckSequenceVerify", Standard),
        0xb2 => ("OpTxVersion", Introspection),
        0xb3 => ("OpTxInputCount", Introspection),
        0xb4 => ("OpTxOutputCount", Introspection),
        0xb5 => ("OpTxLockTime", Introspection),
        0xb6 => ("OpTxSubnetId", Introspection),
        0xb7 => ("OpTxGas", Introspection),
        0xb8 => ("OpTxPayloadSubstr", Introspection),
        0xb9 => ("OpTxInputIndex", Introspection),
        0xba => ("OpOutpointTxId", Introspection),
        0xbb => ("OpOutpointIndex", Introspection),
        0xbc => ("OpTxInputScriptSigSubstr", Introspection),
        0xbd => ("OpTxInputSeq", Introspection),
        0xbe => ("OpTxInputAmount", Introspection),
        0xbf => ("OpTxInputSpk", Introspection),
        0xc0 => ("OpTxInputDaaScore", Introspection),
        0xc1 => ("OpTxInputIsCoinbase", Introspection),
        0xc2 => ("OpTxOutputAmount", Introspection),
        0xc3 => ("OpTxOutputSpk", Introspection),
        0xc4 => ("OpTxPayloadLen", Introspection),
        0xc5 => ("OpTxInputSpkLen", Introspection),
        0xc6 => ("OpTxInputSpkSubstr", Introspection),
        0xc7 => ("OpTxOutputSpkLen", Introspection),
        0xc8 => ("OpTxOutputSpkSubstr", Introspection),
        0xc9 => ("OpTxInputScriptSigLen", Introspection),
        0xcb => ("OpAuthOutputCount", Covenant),
        0xcc => ("OpAuthOutputIdx", Covenant),
        0xcd => ("OpNum2Bin", Introspection),
        0xce => ("OpBin2Num", Introspection),
        0xcf => ("OpInputCovenantId", Covenant),
        0xd0 => ("OpCovInputCount", Covenant),
        0xd1 => ("OpCovInputIdx", Covenant),
        0xd2 => ("OpCovOutputCount", Covenant),
        0xd3 => ("OpCovOutputIdx", Covenant),
        0xd4 => ("OpChainblockSeqCommit", Covenant),
        0xd5 => ("OpOutputCovenantId", Covenant),
        0xd6 => ("OpOutputAuthorizingInput", Covenant),
        0xd7 => ("OpCheckSigFromStack", Standard),
        0xd8 => ("OpCheckSigFromStackECDSA", Standard),
        0xd9 => ("OpBlake3", Standard),
        0xda => ("OpBlake3WithKey", Standard),
        0xfa => ("OpSmallInteger", Standard),
        0xfb => ("OpPubKeys", Standard),
        0xfd => ("OpPubKeyHash", Standard),
        0xfe => ("OpPubKey", Standard),
        0xff => ("OpInvalidOpCode", Standard),
        _ => ("OpUnknown", Unknown),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disassembles_covenant_style_script() {
        // OpTxInputIndex OpInputCovenantId OpData32 <32B> OpEqualVerify OpTrue
        let mut script = vec![0xb9, 0xcf, 0x20];
        script.extend([0x11; 32]);
        script.extend([0x88, 0x51]);
        let (instructions, truncated) = disassemble(&script);
        assert!(!truncated);
        let names: Vec<_> = instructions.iter().map(|i| i.name).collect();
        assert_eq!(names, ["OpTxInputIndex", "OpInputCovenantId", "OpData", "OpEqualVerify", "OpTrue"]);
        assert_eq!(instructions[1].group, OpGroup::Covenant);
        assert_eq!(instructions[2].data.as_deref(), Some([0x11; 32].as_slice()));
    }

    #[test]
    fn flags_truncated_push() {
        let (instructions, truncated) = disassemble(&[0x4c, 0x20, 0x01]);
        assert!(truncated);
        assert_eq!(instructions.len(), 1);
    }
}
