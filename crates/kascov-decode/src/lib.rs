//! Covenant state decoding. Template-specific decoders are additive; the
//! always-correct fallback is an opcode disassembly of the state script.

pub mod disasm;

use disasm::{disassemble, Instruction, OpGroup};

/// What a decoder could make of a covenant state script.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Decoded {
    /// Name of the decoder that matched ("disasm" for the fallback).
    pub decoder: &'static str,
    pub instructions: Vec<Instruction>,
    pub truncated: bool,
    /// Data pushes, in order — for known templates these are the state fields.
    pub pushes: Vec<Vec<u8>>,
    pub uses_covenant_ops: bool,
    pub uses_zk_ops: bool,
}

pub trait StateDecoder: Send + Sync {
    fn name(&self) -> &'static str;
    /// Return a decode if this decoder recognizes the script template.
    fn decode(&self, spk_version: u16, script: &[u8]) -> Option<Decoded>;
}

/// Fallback: full disassembly. Always succeeds.
pub struct DisasmDecoder;

impl StateDecoder for DisasmDecoder {
    fn name(&self) -> &'static str {
        "disasm"
    }
    fn decode(&self, _spk_version: u16, script: &[u8]) -> Option<Decoded> {
        let (instructions, truncated) = disassemble(script);
        Some(Decoded {
            decoder: self.name(),
            pushes: instructions.iter().filter_map(|i| i.data.clone()).collect(),
            uses_covenant_ops: instructions.iter().any(|i| i.group == OpGroup::Covenant),
            uses_zk_ops: instructions.iter().any(|i| i.group == OpGroup::Zk),
            instructions,
            truncated,
        })
    }
}

/// The committed hash of a canonical Kaspa P2SH script-public-key
/// (`OpBlake2b OpData32 <hash> OpEqual`), if `spk` has that shape.
pub fn p2sh_hash(spk: &[u8]) -> Option<&[u8]> {
    (spk.len() == 35 && spk[0] == 0xaa && spk[1] == 0x20 && spk[34] == 0x87)
        .then(|| &spk[2..34])
}

/// Spend-time reveal: when a P2SH state UTXO is spent, the signature
/// script's final push is the program the covenant actually ran. Returns it
/// only if its blake2b-256 matches the committed hash.
pub fn p2sh_reveal(spk: &[u8], sig_script: &[u8]) -> Option<Vec<u8>> {
    let hash = p2sh_hash(spk)?;
    let (instructions, truncated) = disassemble(sig_script);
    if truncated {
        return None;
    }
    let redeem = instructions.last()?.data.clone()?;
    let digest = blake2b_simd::Params::new().hash_length(32).hash(&redeem);
    (digest.as_bytes() == hash).then_some(redeem)
}

/// Try registered decoders in order, ending with the disassembly fallback.
pub struct Registry {
    decoders: Vec<Box<dyn StateDecoder>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self { decoders: vec![] }
    }
}

impl Registry {
    pub fn register(&mut self, decoder: Box<dyn StateDecoder>) {
        self.decoders.push(decoder);
    }

    pub fn decode(&self, spk_version: u16, script: &[u8]) -> Decoded {
        self.decoders
            .iter()
            .find_map(|d| d.decode(spk_version, script))
            .or_else(|| DisasmDecoder.decode(spk_version, script))
            .expect("disasm fallback always decodes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p2sh_reveal_verifies_and_peels() {
        let redeem = vec![0xb9, 0xcf, 0x51]; // OpTxInputIndex OpInputCovenantId OpTrue
        let hash = blake2b_simd::Params::new().hash_length(32).hash(&redeem);
        let mut spk = vec![0xaa, 0x20];
        spk.extend_from_slice(hash.as_bytes());
        spk.push(0x87);
        // sig script: some witness push, then the redeem script push
        let mut sig = vec![0x02, 0x01, 0x02, 0x03];
        sig.extend_from_slice(&redeem);
        assert_eq!(p2sh_reveal(&spk, &sig), Some(redeem.clone()));

        // wrong redeem → hash mismatch → no reveal
        let mut bad_sig = vec![0x03];
        bad_sig.extend_from_slice(&[0x51, 0x52, 0x53]);
        assert_eq!(p2sh_reveal(&spk, &bad_sig), None);

        // non-P2SH spk → no reveal
        assert_eq!(p2sh_reveal(&[0xac], &sig), None);
    }
}
