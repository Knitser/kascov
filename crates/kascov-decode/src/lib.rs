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
