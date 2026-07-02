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

/// One position in a compiled-contract skeleton. SilverScript inlines
/// constructor arguments at their use sites (an argument can appear several
/// times mid-script), so templates are matched on the disassembled
/// instruction stream: fixed opcodes and constant pushes must be identical,
/// argument slots accept any push and yield labeled fields.
enum SkelItem {
    /// A non-push instruction that must match exactly.
    Op(u8),
    /// A push whose bytes are part of the template itself.
    ConstPush(Vec<u8>),
    /// A push carrying a constructor argument.
    Slot(&'static str),
}

pub struct Skeleton {
    pub name: &'static str,
    items: Vec<SkelItem>,
    /// Field labels in constructor order (for display ordering).
    param_order: Vec<&'static str>,
}

/// A push instruction's value, whether it's a data push or a small-int
/// opcode (`OpFalse`/`Op1Negate`/`Op1..Op16`), in script-number encoding.
fn push_value(inst: &Instruction) -> Option<Vec<u8>> {
    if let Some(data) = &inst.data {
        return Some(data.clone());
    }
    match inst.opcode {
        0x00 => Some(vec![]),
        0x4f => Some(vec![0x81]),
        0x51..=0x60 => Some(vec![inst.opcode - 0x50]),
        _ => None,
    }
}

fn is_push(inst: &Instruction) -> bool {
    inst.group == OpGroup::Push
}

impl Skeleton {
    /// Derive a skeleton from two builds of the same contract with different
    /// sentinel arguments. Instructions must align one-to-one: equal
    /// non-push opcodes stay fixed, equal pushes become constants, and
    /// differing pushes become slots — labeled by looking the first build's
    /// value up in `sentinels` (constructor order).
    pub fn derive(
        name: &'static str,
        a: &[u8],
        b: &[u8],
        sentinels: &[(&'static str, Vec<u8>)],
    ) -> Option<Skeleton> {
        let (ia, ta) = disassemble(a);
        let (ib, tb) = disassemble(b);
        if ta || tb || ia.len() != ib.len() {
            return None;
        }
        let mut items = Vec::with_capacity(ia.len());
        for (x, y) in ia.iter().zip(&ib) {
            match (is_push(x), is_push(y)) {
                (false, false) => {
                    if x.opcode != y.opcode {
                        return None;
                    }
                    items.push(SkelItem::Op(x.opcode));
                }
                (true, true) => {
                    let vx = push_value(x)?;
                    let vy = push_value(y)?;
                    if vx == vy {
                        items.push(SkelItem::ConstPush(vx));
                    } else {
                        let (label, _) = sentinels.iter().find(|(_, s)| *s == vx)?;
                        items.push(SkelItem::Slot(label));
                    }
                }
                _ => return None,
            }
        }
        Some(Skeleton {
            name,
            items,
            param_order: sentinels.iter().map(|(l, _)| *l).collect(),
        })
    }

    /// Match a script against this skeleton; on success return its fields in
    /// constructor order. Repeated slots of the same argument must agree.
    fn match_script(&self, instructions: &[Instruction]) -> Option<Vec<Field>> {
        if instructions.len() != self.items.len() {
            return None;
        }
        let mut values: Vec<(&'static str, Vec<u8>)> = Vec::new();
        for (item, inst) in self.items.iter().zip(instructions) {
            match item {
                SkelItem::Op(op) => {
                    if is_push(inst) || inst.opcode != *op {
                        return None;
                    }
                }
                SkelItem::ConstPush(bytes) => {
                    if push_value(inst).as_ref() != Some(bytes) {
                        return None;
                    }
                }
                SkelItem::Slot(label) => {
                    let v = push_value(inst)?;
                    match values.iter().find(|(l, _)| l == label) {
                        Some((_, prev)) if *prev != v => return None,
                        Some(_) => {}
                        None => values.push((label, v)),
                    }
                }
            }
        }
        Some(
            self.param_order
                .iter()
                .filter_map(|label| {
                    values
                        .iter()
                        .find(|(l, _)| l == label)
                        .map(|(_, v)| Field { name: label, value: v.clone() })
                })
                .collect(),
        )
    }
}

/// Matches compiled contracts against known skeletons.
pub struct TemplateDecoder {
    skeletons: Vec<Skeleton>,
}

impl TemplateDecoder {
    pub fn new(skeletons: Vec<Skeleton>) -> Self {
        Self { skeletons }
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
        for skel in &self.skeletons {
            if let Some(fields) = skel.match_script(&instructions) {
                let mut d = base_decode("template", script);
                d.template = Some(skel.name);
                d.fields = fields;
                return Some(d);
            }
        }
        None
    }
}

/* ------------------------------------------------ SilverScript templates
   The example contracts from kaspanet/silverscript
   (silverscript-lang/tests/examples), each compiled twice with sentinel
   constructor arguments via `compile_contract` — skeletons derive at
   registration and stay aligned with these exact dumps. */

const SENT_A32: [u8; 32] = [0x11; 32];
const SENT_B32: [u8; 32] = [0x22; 32];
const SENT_C32: [u8; 32] = [0x33; 32];
const SENT_D32: [u8; 32] = [0x44; 32];

const MECENAS_A: &str = "6b6c76009c637502e803b100c3201111111111111111111111111111111111111111111111111111111111111111030000207c7e01ac7e876902e803b9be760400e1f50594527994760400e1f505547993a16300c252795479949c696700c20400e1f5059c6951c3b9bf876951c2789c6968007a75007a75007a75516776519c637578aa2033333333333333333333333333333333333333333333333333333333333333338769765279ac69757551677500696868";
const MECENAS_B: &str = "6b6c76009c637502d007b100c3202222222222222222222222222222222222222222222222222222222222222222030000207c7e01ac7e876902e803b9be760480b2e60e94527994760480b2e60e547993a16300c252795479949c696700c20480b2e60e9c6951c3b9bf876951c2789c6968007a75007a75007a75516776519c637578aa2044444444444444444444444444444444444444444444444444444444444444448769765279ac69757551677500696868";
const ESCROW_A: &str = "78aa2033333333333333333333333333333333333333333333333333333333333333338769765279ac6900c2b9be02e803949c6900c3201111111111111111111111111111111111111111111111111111111111111111030000207c7e01ac7e8700c3202222222222222222222222222222222222222222222222222222222222222222030000207c7e01ac7e879b69757551";
const ESCROW_B: &str = "78aa2044444444444444444444444444444444444444444444444444444444444444448769765279ac6900c2b9be02e803949c6900c3202222222222222222222222222222222222222222222222222222222222222222030000207c7e01ac7e8700c3201111111111111111111111111111111111111111111111111111111111111111030000207c7e01ac7e879b69757551";
const LASTWILL_A: &str = "6b6c76009c637502b400b178aa2033333333333333333333333333333333333333333333333333333333333333338769765279ac697575516776519c637578aa2044444444444444444444444444444444444444444444444444444444444444448769765279ac697575516776529c637578aa2011111111111111111111111111111111111111111111111111111111111111118769765279ac6900c2b9be02e803949c6900c3b9bf876975755167750069686868";
const LASTWILL_B: &str = "6b6c76009c637502b400b178aa2044444444444444444444444444444444444444444444444444444444444444448769765279ac697575516776519c637578aa2033333333333333333333333333333333333333333333333333333333333333338769765279ac697575516776529c637578aa2022222222222222222222222222222222222222222222222222222222222222228769765279ac6900c2b9be02e803949c6900c3b9bf876975755167750069686868";

/// Minimal script-number encoding of the sentinel ints used in the dumps.
fn snum(v: i64) -> Vec<u8> {
    let mut out = Vec::new();
    let mut abs = v.unsigned_abs();
    while abs > 0 {
        out.push((abs & 0xff) as u8);
        abs >>= 8;
    }
    if let Some(last) = out.last() {
        if last & 0x80 != 0 {
            out.push(0);
        }
    }
    out
}

fn silverscript_skeletons() -> Vec<Skeleton> {
    let hex2 = |s: &str| hex::decode(s).expect("template dump hex");
    let mut out = Vec::new();
    // contract Mecenas(pubkey recipient, byte[32] funder, int pledge, int period)
    if let Some(s) = Skeleton::derive(
        "SilverScript · Mecenas",
        &hex2(MECENAS_A),
        &hex2(MECENAS_B),
        &[
            ("recipient", SENT_A32.to_vec()),
            ("funder_hash", SENT_C32.to_vec()),
            ("pledge", snum(100_000_000)),
            ("period", snum(1000)),
        ],
    ) {
        out.push(s);
    }
    // contract Escrow(byte[32] arbiter, pubkey buyer, pubkey seller)
    if let Some(s) = Skeleton::derive(
        "SilverScript · Escrow",
        &hex2(ESCROW_A),
        &hex2(ESCROW_B),
        &[
            ("arbiter_hash", SENT_C32.to_vec()),
            ("buyer", SENT_A32.to_vec()),
            ("seller", SENT_B32.to_vec()),
        ],
    ) {
        out.push(s);
    }
    // contract LastWill(byte[32] inheritor, byte[32] cold, byte[32] hot)
    if let Some(s) = Skeleton::derive(
        "SilverScript · LastWill",
        &hex2(LASTWILL_A),
        &hex2(LASTWILL_B),
        &[
            ("inheritor_hash", SENT_C32.to_vec()),
            ("cold_hash", SENT_D32.to_vec()),
            ("hot_hash", SENT_A32.to_vec()),
        ],
    ) {
        out.push(s);
    }
    out
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
                Box::new(TemplateDecoder::new(silverscript_skeletons())),
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
    fn all_silverscript_skeletons_derive() {
        let names: Vec<_> = silverscript_skeletons().iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            [
                "SilverScript · Mecenas",
                "SilverScript · Escrow",
                "SilverScript · LastWill"
            ],
            "every embedded compiler dump must derive a skeleton"
        );
    }

    #[test]
    fn skeleton_matches_real_compiled_instances_and_labels_args() {
        let reg = Registry::default();

        // Mecenas instance B: sentinel args flipped vs A
        let d = reg.decode(0, &hex::decode(MECENAS_B).unwrap());
        assert_eq!(d.template, Some("SilverScript · Mecenas"));
        let get = |n: &str| d.fields.iter().find(|f| f.name == n).map(|f| f.value.clone());
        assert_eq!(get("recipient"), Some(vec![0x22; 32]));
        assert_eq!(get("funder_hash"), Some(vec![0x44; 32]));
        assert_eq!(get("pledge"), Some(snum(250_000_000)));
        assert_eq!(get("period"), Some(snum(2000)));

        // Escrow A: arbiter/buyer/seller land on the right labels even though
        // buyer/seller swap between the two builds
        let d = reg.decode(0, &hex::decode(ESCROW_A).unwrap());
        assert_eq!(d.template, Some("SilverScript · Escrow"));
        let get = |n: &str| d.fields.iter().find(|f| f.name == n).map(|f| f.value.clone());
        assert_eq!(get("arbiter_hash"), Some(vec![0x33; 32]));
        assert_eq!(get("buyer"), Some(vec![0x11; 32]));
        assert_eq!(get("seller"), Some(vec![0x22; 32]));

        // LastWill B
        let d = reg.decode(0, &hex::decode(LASTWILL_B).unwrap());
        assert_eq!(d.template, Some("SilverScript · LastWill"));

        // one flipped opcode → no template, falls back to plain disasm
        let mut broken = hex::decode(MECENAS_A).unwrap();
        let last = broken.len() - 1;
        broken[last] = 0x51;
        let d = reg.decode(0, &broken);
        assert_eq!(d.template, None);
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
