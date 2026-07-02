//! Covenant state decoding. Template-specific decoders are additive; the
//! always-correct fallback is an opcode disassembly of the state script.

pub mod disasm;

use disasm::{disassemble, Instruction, OpGroup};

/// A labeled state field extracted by a template decoder.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Field {
    pub name: &'static str,
    #[serde(serialize_with = "hex_ser")]
    pub value: Vec<u8>,
}

fn hex_ser<S: serde::Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&hex::encode(bytes))
}

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
    /// Recognized template name, when a template decoder matched.
    pub template: Option<&'static str>,
    /// Labeled constructor/state fields, when the template names them.
    pub fields: Vec<Field>,
}

pub trait StateDecoder: Send + Sync {
    fn name(&self) -> &'static str;
    /// Return a decode if this decoder recognizes the script template.
    fn decode(&self, spk_version: u16, script: &[u8]) -> Option<Decoded>;
}

/// Fallback: full disassembly. Always succeeds.
pub struct DisasmDecoder;

fn base_decode(name: &'static str, script: &[u8]) -> Decoded {
    let (instructions, truncated) = disassemble(script);
    Decoded {
        decoder: name,
        pushes: instructions.iter().filter_map(|i| i.data.clone()).collect(),
        uses_covenant_ops: instructions.iter().any(|i| i.group == OpGroup::Covenant),
        uses_zk_ops: instructions.iter().any(|i| i.group == OpGroup::Zk),
        instructions,
        truncated,
        template: None,
        fields: vec![],
    }
}

impl StateDecoder for DisasmDecoder {
    fn name(&self) -> &'static str {
        "disasm"
    }
    fn decode(&self, _spk_version: u16, script: &[u8]) -> Option<Decoded> {
        Some(base_decode(self.name(), script))
    }
}

/// `<push 32/33 bytes> OpCheckSig` — the plain pay-to-pubkey state carried by
/// most covenants observed on TN10 (and the [[Covenant Lab]] ones).
pub struct P2pkStateDecoder;

impl StateDecoder for P2pkStateDecoder {
    fn name(&self) -> &'static str {
        "p2pk-state"
    }
    fn decode(&self, _spk_version: u16, script: &[u8]) -> Option<Decoded> {
        let ok = matches!(script.len(), 34 | 35)
            && script[0] as usize == script.len() - 2
            && script[script.len() - 1] == 0xac;
        if !ok {
            return None;
        }
        let mut d = base_decode(self.name(), script);
        d.template = Some("p2pk state");
        d.fields = vec![Field { name: "owner_pubkey", value: script[1..script.len() - 1].to_vec() }];
        Some(d)
    }
}

/// `OpBlake2b <32-byte hash> OpEqual` — a P2SH commitment: the program is
/// revealed at spend time (see `p2sh_reveal`).
pub struct P2shCommitmentDecoder;

impl StateDecoder for P2shCommitmentDecoder {
    fn name(&self) -> &'static str {
        "p2sh-commitment"
    }
    fn decode(&self, _spk_version: u16, script: &[u8]) -> Option<Decoded> {
        let hash = p2sh_hash(script)?.to_vec();
        let mut d = base_decode(self.name(), script);
        d.template = Some("p2sh commitment");
        d.fields = vec![Field { name: "program_hash", value: hash }];
        Some(d)
    }
}

/// A recognizable compiled-contract shape: leading data pushes (the
/// constructor arguments, labeled) followed by an invariant body.
pub struct Template {
    pub name: &'static str,
    /// Labels for the leading pushes, in on-script order.
    pub params: &'static [&'static str],
    pub body: Vec<u8>,
}

impl Template {
    /// Split two instances of the same compiled contract (built with
    /// different constructor arguments) into their invariant body: the
    /// longest common byte suffix that begins on an instruction boundary in
    /// both and is preceded only by data pushes.
    pub fn derive_body(a: &[u8], b: &[u8]) -> Option<Vec<u8>> {
        let boundaries = |script: &[u8]| -> Option<Vec<usize>> {
            let (ins, truncated) = disassemble(script);
            if truncated {
                return None;
            }
            Some(ins.iter().map(|i| i.offset).chain([script.len()]).collect())
        };
        let ba = boundaries(a)?;
        let bb = boundaries(b)?;
        // longest common suffix in bytes
        let mut k = 0;
        while k < a.len() && k < b.len() && a[a.len() - 1 - k] == b[b.len() - 1 - k] {
            k += 1;
        }
        // largest instruction boundary in both that the suffix covers
        let split_a = ba.iter().copied().find(|&off| a.len() - off <= k && bb.contains(&(b.len() - (a.len() - off))))?;
        let body = a[split_a..].to_vec();
        // the leading remainder must be pure pushes in both instances
        let leading_ok = |script: &[u8], split: usize| {
            let (ins, _) = disassemble(script);
            ins.iter().take_while(|i| i.offset < split).all(|i| i.data.is_some() || i.opcode == 0x00 || (0x4f..=0x60).contains(&i.opcode))
        };
        (leading_ok(a, split_a) && leading_ok(b, b.len() - body.len()) && !body.is_empty()).then_some(body)
    }
}

/// Matches compiled contracts by invariant body suffix and labels their
/// leading constructor pushes.
pub struct TemplateDecoder {
    templates: Vec<Template>,
}

impl TemplateDecoder {
    pub fn new(templates: Vec<Template>) -> Self {
        Self { templates }
    }
}

impl StateDecoder for TemplateDecoder {
    fn name(&self) -> &'static str {
        "template"
    }
    fn decode(&self, _spk_version: u16, script: &[u8]) -> Option<Decoded> {
        let (instructions, truncated) = disassemble(script);
        if truncated {
            return None;
        }
        for t in &self.templates {
            if t.body.is_empty() || script.len() < t.body.len() {
                continue;
            }
            let split = script.len() - t.body.len();
            if script[split..] != t.body[..] {
                continue;
            }
            let lead: Vec<_> = instructions.iter().take_while(|i| i.offset < split).collect();
            // leading part must be exactly the constructor pushes, ending at the split
            let lead_end = lead.last().map(|_| {
                instructions.iter().find(|i| i.offset >= split).map(|i| i.offset).unwrap_or(script.len())
            });
            if lead.len() != t.params.len() || lead_end != Some(split) {
                continue;
            }
            let small_int = |op: u8| -> Option<Vec<u8>> {
                match op {
                    0x00 => Some(vec![0]),
                    0x4f => Some(vec![0x81]), // -1, script-number encoding
                    0x51..=0x60 => Some(vec![op - 0x50]),
                    _ => None,
                }
            };
            let mut fields = Vec::with_capacity(lead.len());
            for (param, inst) in t.params.iter().zip(&lead) {
                let value = inst.data.clone().or_else(|| small_int(inst.opcode))?;
                fields.push(Field { name: param, value });
            }
            let mut d = base_decode("template", script);
            d.template = Some(t.name);
            d.fields = fields;
            return Some(d);
        }
        None
    }
}

/// SilverScript example contracts (kaspanet/silverscript,
/// silverscript-lang/tests/examples). Bodies are the invariant compiled
/// bytecode derived from two differently-parameterized builds via
/// `Template::derive_body`; params are labeled by matching sentinel
/// constructor values. Empty bodies are placeholders awaiting regeneration
/// (see docs/Decoding.md) and are skipped at registration.
const SILVERSCRIPT_TEMPLATES: &[(&str, &[&str], &str)] = &[
    ("SilverScript · Mecenas", &["recipient", "funder_hash", "pledge", "period"], ""),
    ("SilverScript · Escrow", &["arbiter_hash", "buyer", "seller"], ""),
    ("SilverScript · LastWill", &["inheritor_hash", "cold_hash", "hot_hash"], ""),
];

fn silverscript_templates() -> Vec<Template> {
    SILVERSCRIPT_TEMPLATES
        .iter()
        .filter(|(_, _, body)| !body.is_empty())
        .map(|(name, params, body)| Template {
            name,
            params,
            body: hex::decode(body).expect("template body hex"),
        })
        .collect()
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
        Self {
            decoders: vec![
                Box::new(TemplateDecoder::new(silverscript_templates())),
                Box::new(P2pkStateDecoder),
                Box::new(P2shCommitmentDecoder),
            ],
        }
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
    fn p2pk_state_labels_owner() {
        let mut script = vec![0x20];
        script.extend([0x7f; 32]);
        script.push(0xac);
        let d = Registry::default().decode(0, &script);
        assert_eq!(d.template, Some("p2pk state"));
        assert_eq!(d.fields.len(), 1);
        assert_eq!(d.fields[0].name, "owner_pubkey");
        assert_eq!(d.fields[0].value, vec![0x7f; 32]);
    }

    #[test]
    fn p2sh_commitment_labels_hash() {
        let mut script = vec![0xaa, 0x20];
        script.extend([0x42; 32]);
        script.push(0x87);
        let d = Registry::default().decode(0, &script);
        assert_eq!(d.template, Some("p2sh commitment"));
        assert_eq!(d.fields[0].name, "program_hash");
        assert_eq!(d.fields[0].value, vec![0x42; 32]);
    }

    #[test]
    fn template_matches_and_labels_constructor_pushes() {
        // synthetic "contract": <argB> <argA> OpTxInputIndex OpInputCovenantId OpEqualVerify OpTrue
        let body = vec![0xb9, 0xcf, 0x88, 0x51];
        let instance = |a: u8, b: u8| {
            let mut s = vec![0x20];
            s.extend([b; 32]);
            s.extend([0x04, a, a, a, a]);
            s.extend(&body);
            s
        };
        let derived = Template::derive_body(&instance(0x11, 0x33), &instance(0x22, 0x44))
            .expect("derives the invariant body");
        assert_eq!(derived, body);

        let dec = TemplateDecoder::new(vec![Template {
            name: "synthetic",
            params: &["big_field", "small_field"],
            body: derived,
        }]);
        let d = dec.decode(0, &instance(0x55, 0x66)).expect("template matches");
        assert_eq!(d.template, Some("synthetic"));
        assert_eq!(d.fields[0].name, "big_field");
        assert_eq!(d.fields[0].value, vec![0x66; 32]);
        assert_eq!(d.fields[1].name, "small_field");
        assert_eq!(d.fields[1].value, vec![0x55; 4]);

        // a script with a different tail must not match
        let mut other = instance(0x55, 0x66);
        *other.last_mut().unwrap() = 0x52;
        assert!(dec.decode(0, &other).is_none());
    }

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
