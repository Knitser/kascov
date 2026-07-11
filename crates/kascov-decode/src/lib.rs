//! Covenant state decoding. Template-specific decoders are additive; the
//! always-correct fallback is an opcode disassembly of the state script.

pub mod disasm;
pub mod observed;

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
    /// Best-effort proving system guessed from the ZK arguments, when the
    /// script uses `OpZkPrecompile` (see `zk_system`). `None` when there are
    /// no ZK ops or the shape is too ambiguous to call.
    pub zk_system: Option<&'static str>,
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
        zk_system: zk_system_from(&instructions),
        instructions,
        truncated,
        template: None,
        fields: vec![],
    }
}

/// Guess which zero-knowledge proving system a covenant script hands to
/// `OpZkPrecompile` (KIP-16, opcode `0xa6`). Best effort: the verifier pops a
/// verifying key, a proof, and public inputs off the stack, so the data
/// pushes in the program encode those shapes. We key on the two systems'
/// very different proof sizes and prefer `None` over a shaky guess.
///
///   * **Groth16** (BN254 / BLS12-381) has a fixed, tiny proof — three curve
///     points, 128–256 bytes depending on curve and compression — and its
///     public inputs are 32-byte field elements. The verifying key is a
///     handful of 64-byte G1 / 96–128-byte G2 points. No single push reaches
///     STARK scale.
///   * **RISC Zero** seals are STARK receipts: kilobytes of proof data plus a
///     32-byte image id. A push of >= 1 KiB feeding the precompile is the tell.
///   * A push between those bands (300–1023 bytes) is bigger than any Groth16
///     encoding but below STARK scale — some succinct system we can't
///     attribute further, labeled "succinct proof (inferred)".
///
/// Bare 32/64-byte pushes on their own are too generic to attribute, so those
/// yield `None` (as does the 257–299 byte gap just above the Groth16 band).
pub fn zk_system(script: &[u8]) -> Option<&'static str> {
    let (instructions, _) = disassemble(script);
    zk_system_from(&instructions)
}

fn zk_system_from(instructions: &[Instruction]) -> Option<&'static str> {
    // Only meaningful for scripts that actually invoke the ZK verifier.
    if !instructions.iter().any(|i| i.group == OpGroup::Zk) {
        return None;
    }
    let sizes: Vec<usize> = instructions.iter().filter_map(|i| i.data.as_ref().map(|d| d.len())).collect();
    // STARK-scale seal → RISC Zero.
    if sizes.iter().any(|&n| n >= 1024) {
        return Some("risc0");
    }
    // Groth16 proof band: three curve points, ~128 (compressed) up to 256
    // (uncompressed). A push in this range, with nothing STARK-scale present,
    // reads as Groth16.
    if sizes.iter().any(|&n| (128..=256).contains(&n)) {
        return Some("groth16");
    }
    // Above every Groth16 encoding but below STARK scale: some succinct
    // system's proof, unattributable beyond that.
    if sizes.iter().any(|&n| (300..1024).contains(&n)) {
        return Some("succinct proof (inferred)");
    }
    None
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
    /// A push whose bytes are part of the template itself. `raw` keeps the
    /// original encoding so re-emitting a contract is byte-identical even
    /// where the compiler chose a non-minimal push.
    ConstPush { value: Vec<u8>, raw: Vec<u8> },
    /// A push carrying a constructor argument.
    Slot(&'static str),
}

/// Canonical push encoding — the bytes the SilverScript compiler's
/// ScriptBuilder emits for a pushed value. Mirrors `encodePush` in
/// `web/disasm.js`; used to re-encode argument slots when emitting a contract.
pub fn encode_push(value: &[u8]) -> Vec<u8> {
    match value {
        [] => vec![0x00],                              // OpFalse
        [0x81] => vec![0x4f],                          // Op1Negate
        [v] if (1..=16).contains(v) => vec![0x50 + v], // Op1..Op16
        _ if value.len() <= 75 => {
            let mut out = Vec::with_capacity(value.len() + 1);
            out.push(value.len() as u8);
            out.extend_from_slice(value);
            out
        }
        _ if value.len() <= 0xff => {
            let mut out = vec![0x4c, value.len() as u8];
            out.extend_from_slice(value);
            out
        }
        _ if value.len() <= 0xffff => {
            let mut out = vec![0x4d, (value.len() & 0xff) as u8, (value.len() >> 8) as u8];
            out.extend_from_slice(value);
            out
        }
        _ => {
            let n = value.len() as u32;
            let mut out = vec![0x4e, (n & 0xff) as u8, (n >> 8 & 0xff) as u8, (n >> 16 & 0xff) as u8, (n >> 24) as u8];
            out.extend_from_slice(value);
            out
        }
    }
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
        for (i, (x, y)) in ia.iter().zip(&ib).enumerate() {
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
                        // raw span of this push in dump A, for byte-perfect emit
                        let end = ia.get(i + 1).map_or(a.len(), |n| n.offset);
                        items.push(SkelItem::ConstPush { value: vx, raw: a[x.offset..end].to_vec() });
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

    /// Derive a skeleton from two or more distinct on-chain instances of the
    /// same compiled contract (no sentinels available — the arguments are
    /// whatever the deployers used). Instructions must align one-to-one
    /// across every instance: equal non-push opcodes stay fixed, pushes that
    /// agree everywhere become constants, and pushes that differ anywhere
    /// become slots. Slots are labeled in first-occurrence order; two slot
    /// positions whose values agree in *every* instance are the same inlined
    /// argument and share one label (and `match_script` will keep enforcing
    /// that they agree). `labels` must name exactly the distinct slots.
    pub fn derive_observed(
        name: &'static str,
        instances: &[&[u8]],
        labels: &[&'static str],
    ) -> Option<Skeleton> {
        if instances.len() < 2 {
            return None;
        }
        let mut streams = Vec::with_capacity(instances.len());
        for bytes in instances {
            let (insts, truncated) = disassemble(bytes);
            if truncated || streams.first().is_some_and(|f: &Vec<Instruction>| f.len() != insts.len()) {
                return None;
            }
            streams.push(insts);
        }
        let first = &streams[0];
        let mut items = Vec::with_capacity(first.len());
        // Distinct slots seen so far, as their value-vector across instances.
        let mut slots: Vec<Vec<Vec<u8>>> = Vec::new();
        for (i, inst) in first.iter().enumerate() {
            if !is_push(inst) {
                if streams.iter().any(|s| is_push(&s[i]) || s[i].opcode != inst.opcode) {
                    return None;
                }
                items.push(SkelItem::Op(inst.opcode));
                continue;
            }
            let vector = streams
                .iter()
                .map(|s| push_value(&s[i]))
                .collect::<Option<Vec<_>>>()?;
            if vector.iter().all(|v| *v == vector[0]) {
                let end = first.get(i + 1).map_or(instances[0].len(), |n| n.offset);
                items.push(SkelItem::ConstPush {
                    value: vector[0].clone(),
                    raw: instances[0][inst.offset..end].to_vec(),
                });
            } else {
                let slot = match slots.iter().position(|s| *s == vector) {
                    Some(idx) => idx,
                    None => {
                        slots.push(vector);
                        slots.len() - 1
                    }
                };
                items.push(SkelItem::Slot(labels.get(slot).copied()?));
            }
        }
        // Every label must correspond to an actual slot — a mismatch means
        // the fixtures (or the labels) are wrong.
        if slots.len() != labels.len() {
            return None;
        }
        Some(Skeleton { name, items, param_order: labels.to_vec() })
    }

    /// Constructor parameter labels, in order.
    pub fn params(&self) -> &[&'static str] {
        &self.param_order
    }

    /// Re-emit this contract's compiled bytes with new constructor arguments.
    /// Fixed ops and constant pushes stay byte-identical to the original
    /// build; each slot is re-encoded from `args` (looked up by label).
    /// Returns None if an argument is missing. The inverse of `match_script`:
    /// `match_script(disassemble(emit(args))) == args`.
    pub fn emit(&self, args: &[(&str, &[u8])]) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        for item in &self.items {
            match item {
                SkelItem::Op(op) => out.push(*op),
                SkelItem::ConstPush { raw, .. } => out.extend_from_slice(raw),
                SkelItem::Slot(label) => {
                    let (_, value) = args.iter().find(|(l, _)| l == label)?;
                    out.extend_from_slice(&encode_push(value));
                }
            }
        }
        Some(out)
    }

    /// Match a script against this skeleton; on success return its fields in
    /// constructor order. Repeated slots of the same argument must agree.
    fn match_script(&self, instructions: &[Instruction]) -> Option<Vec<Field>> {
        if instructions.len() != self.items.len() {
            return None;
        }
        let mut values: Vec<(&'static str, Vec<u8>)> = Vec::new();
        for (item, inst) in self.items.iter().zip(instructions) {
            if !match_skel_item(item, inst, &mut values) {
                return None;
            }
        }
        Some(fields_in_order(&self.param_order, &values))
    }
}

/// Match one skeleton item against one instruction. Slot values accumulate in
/// `values`; a label seen twice within the same scope must carry the same
/// value (SilverScript inlines an argument at every use site).
fn match_skel_item(
    item: &SkelItem,
    inst: &Instruction,
    values: &mut Vec<(&'static str, Vec<u8>)>,
) -> bool {
    match item {
        SkelItem::Op(op) => !is_push(inst) && inst.opcode == *op,
        SkelItem::ConstPush { value, .. } => push_value(inst).as_ref() == Some(value),
        SkelItem::Slot(label) => {
            let Some(v) = push_value(inst) else { return false };
            match values.iter().find(|(l, _)| l == label) {
                Some((_, prev)) => *prev == v,
                None => {
                    values.push((label, v));
                    true
                }
            }
        }
    }
}

fn fields_in_order(order: &[&'static str], values: &[(&'static str, Vec<u8>)]) -> Vec<Field> {
    order
        .iter()
        .filter_map(|label| {
            values.iter().find(|(l, _)| l == label).map(|(_, v)| Field { name: label, value: v.clone() })
        })
        .collect()
}

/// Push-size-aware shape equality: two instructions align when both are
/// pushes of the same width or both are the same non-push opcode. This is
/// the alignment used to find the repeated block of a variable-arity family.
fn same_shape(a: &Instruction, b: &Instruction) -> bool {
    match (push_value(a), push_value(b)) {
        (Some(x), Some(y)) => x.len() == y.len(),
        (None, None) => a.opcode == b.opcode,
        _ => false,
    }
}

/// A compiled-contract family whose builds differ only by how many times one
/// instruction block repeats (e.g. genesis0's slot-mint emits one
/// amount+script check per collection output). Matched as
/// `prefix · group×N · suffix` with `N >= min_repeats`; the group's pushes
/// are per-repeat slots, so every arity of the family decodes to one name.
pub struct RepeatSkeleton {
    pub name: &'static str,
    prefix: Vec<SkelItem>,
    group: Vec<SkelItem>,
    suffix: Vec<SkelItem>,
    min_repeats: usize,
    param_order: Vec<&'static str>,
    group_params: Vec<&'static str>,
}

impl RepeatSkeleton {
    /// Derive from real instances of two different arities: `long` holds two
    /// or more instances of the bigger build, `short` at least one of the
    /// smaller. The repeated group is the shape difference between the two
    /// arities (rotated to its leftmost position, so trailing copies inside
    /// the longer build's aligned prefix fold into repeats). Fixed-part
    /// pushes become constants only when *every* instance of *both* arities
    /// agrees — arity-dependent constants (output counts, indexes) become
    /// slots automatically. `labels` names the distinct fixed-part slots in
    /// first-occurrence order; `group_labels` names the group's pushes in
    /// order (repeat a label to require equality within one repeat).
    pub fn derive(
        name: &'static str,
        long: &[&[u8]],
        short: &[&[u8]],
        labels: &[&'static str],
        group_labels: &[&'static str],
    ) -> Option<RepeatSkeleton> {
        if long.len() < 2 || short.is_empty() {
            return None;
        }
        let parse = |set: &[&[u8]]| -> Option<Vec<Vec<Instruction>>> {
            let mut streams = Vec::with_capacity(set.len());
            for bytes in set {
                let (insts, truncated) = disassemble(bytes);
                if truncated || streams.first().is_some_and(|f: &Vec<Instruction>| f.len() != insts.len()) {
                    return None;
                }
                streams.push(insts);
            }
            Some(streams)
        };
        let la = parse(long)?;
        let lb = parse(short)?;
        let (a0, b0) = (&la[0], &lb[0]);
        let g = a0.len().checked_sub(b0.len()).filter(|g| *g > 0)?;
        // Shape-align the two arities from both ends, then rotate the group
        // window as far left as it goes so it sits at the first repeat.
        let mut p = 0;
        while p < b0.len() && same_shape(&a0[p], &b0[p]) {
            p += 1;
        }
        let mut s = 0;
        while s < b0.len() - p && same_shape(&a0[a0.len() - 1 - s], &b0[b0.len() - 1 - s]) {
            s += 1;
        }
        if a0.len() - p - s < g {
            return None;
        }
        p = a0.len() - s - g; // group directly before the suffix…
        while p > 0 && same_shape(&a0[p - 1], &a0[p + g - 1]) {
            p -= 1; // …rotated to its leftmost equivalent position
        }
        let extra = b0.len().checked_sub(p + s)?;
        if extra % g != 0 {
            return None;
        }
        let min_repeats = extra / g;

        // Fixed parts: const/slot decided across every instance of both
        // arities (suffix positions aligned from the end).
        let mut slots: Vec<Vec<Vec<u8>>> = Vec::new();
        let mut build = |positions: &mut dyn Iterator<Item = (usize, usize)>| -> Option<Vec<SkelItem>> {
            let mut items = Vec::new();
            for (ia, ib) in positions {
                let inst = &a0[ia];
                if !is_push(inst) {
                    let ok = la.iter().all(|x| !is_push(&x[ia]) && x[ia].opcode == inst.opcode)
                        && lb.iter().all(|x| !is_push(&x[ib]) && x[ib].opcode == inst.opcode);
                    if !ok {
                        return None;
                    }
                    items.push(SkelItem::Op(inst.opcode));
                    continue;
                }
                let vector = la
                    .iter()
                    .map(|x| push_value(&x[ia]))
                    .chain(lb.iter().map(|x| push_value(&x[ib])))
                    .collect::<Option<Vec<_>>>()?;
                if vector.iter().all(|v| *v == vector[0]) {
                    let end = a0.get(ia + 1).map_or(long[0].len(), |n| n.offset);
                    items.push(SkelItem::ConstPush {
                        value: vector[0].clone(),
                        raw: long[0][inst.offset..end].to_vec(),
                    });
                } else {
                    let slot = match slots.iter().position(|x| *x == vector) {
                        Some(idx) => idx,
                        None => {
                            slots.push(vector);
                            slots.len() - 1
                        }
                    };
                    items.push(SkelItem::Slot(labels.get(slot).copied()?));
                }
            }
            Some(items)
        };
        let prefix = build(&mut (0..p).map(|i| (i, i)))?;
        let suffix = build(&mut (0..s).map(|j| (a0.len() - s + j, b0.len() - s + j)))?;
        if slots.len() != labels.len() {
            return None;
        }

        // The group: every push is a per-repeat slot.
        let mut group = Vec::with_capacity(g);
        let mut pushes = 0;
        for inst in &a0[p..p + g] {
            if is_push(inst) {
                group.push(SkelItem::Slot(*group_labels.get(pushes)?));
                pushes += 1;
            } else {
                group.push(SkelItem::Op(inst.opcode));
            }
        }
        if pushes != group_labels.len() {
            return None;
        }
        let mut group_params: Vec<&'static str> = Vec::new();
        for label in group_labels {
            if !group_params.contains(label) {
                group_params.push(label);
            }
        }

        let skel = RepeatSkeleton {
            name,
            prefix,
            group,
            suffix,
            min_repeats,
            param_order: labels.to_vec(),
            group_params,
        };
        // Nothing is registered on faith: the derived matcher must accept
        // every instance it was derived from.
        for bytes in long.iter().chain(short) {
            let (insts, _) = disassemble(bytes);
            skel.match_script(&insts)?;
        }
        Some(skel)
    }

    /// Fixed-part parameter labels, in order.
    pub fn params(&self) -> &[&'static str] {
        &self.param_order
    }

    /// Per-repeat parameter labels, in order.
    pub fn group_params(&self) -> &[&'static str] {
        &self.group_params
    }

    /// Match `prefix · group×N · suffix`; fixed-part fields come first (in
    /// `params()` order), then each repeat's fields in repeat order.
    fn match_script(&self, instructions: &[Instruction]) -> Option<Vec<Field>> {
        let fixed = self.prefix.len() + self.suffix.len();
        let extra = instructions.len().checked_sub(fixed)?;
        if self.group.is_empty() || extra % self.group.len() != 0 {
            return None;
        }
        let repeats = extra / self.group.len();
        if repeats < self.min_repeats {
            return None;
        }
        let mut values: Vec<(&'static str, Vec<u8>)> = Vec::new();
        for (item, inst) in self.prefix.iter().zip(instructions) {
            if !match_skel_item(item, inst, &mut values) {
                return None;
            }
        }
        let mut at = self.prefix.len();
        let mut repeat_fields: Vec<Field> = Vec::new();
        for _ in 0..repeats {
            // Fresh scope per repeat: a repeated label must agree within one
            // repeat (an output index used twice) but may differ across them.
            let mut rv: Vec<(&'static str, Vec<u8>)> = Vec::new();
            for item in &self.group {
                if !match_skel_item(item, &instructions[at], &mut rv) {
                    return None;
                }
                at += 1;
            }
            repeat_fields.extend(fields_in_order(&self.group_params, &rv));
        }
        for (item, inst) in self.suffix.iter().zip(&instructions[at..]) {
            if !match_skel_item(item, inst, &mut values) {
                return None;
            }
        }
        let mut fields = fields_in_order(&self.param_order, &values);
        fields.append(&mut repeat_fields);
        Some(fields)
    }
}

/// Matches compiled contracts against known skeletons.
pub struct TemplateDecoder {
    skeletons: Vec<Skeleton>,
    repeats: Vec<RepeatSkeleton>,
}

impl TemplateDecoder {
    pub fn new(skeletons: Vec<Skeleton>) -> Self {
        Self { skeletons, repeats: vec![] }
    }

    pub fn with_repeats(skeletons: Vec<Skeleton>, repeats: Vec<RepeatSkeleton>) -> Self {
        Self { skeletons, repeats }
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
        let hit = self
            .skeletons
            .iter()
            .find_map(|s| s.match_script(&instructions).map(|f| (s.name, f)))
            .or_else(|| {
                self.repeats.iter().find_map(|s| s.match_script(&instructions).map(|f| (s.name, f)))
            });
        let (name, fields) = hit?;
        let mut d = base_decode("template", script);
        d.template = Some(name);
        d.fields = fields;
        Some(d)
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
/// Minimal script-number (little-endian, sign-guard) encoding of a
/// non-negative integer — used for pledge/period args and entrypoint
/// selectors when emitting a contract or building a spend witness.
pub fn snum(v: i64) -> Vec<u8> {
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

pub fn silverscript_skeletons() -> Vec<Skeleton> {
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
        let mut skeletons = silverscript_skeletons();
        skeletons.extend(observed::observed_skeletons());
        Self {
            decoders: vec![
                Box::new(TemplateDecoder::with_repeats(skeletons, observed::observed_repeat_skeletons())),
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
    fn emit_with_sentinel_args_reproduces_each_dump() {
        // emitting with the sentinel args must reproduce dump A byte-for-byte
        let cases: &[(&str, &str, &[(&str, Vec<u8>)])] = &[
            (
                "SilverScript · Mecenas",
                MECENAS_A,
                &[
                    ("recipient", vec![0x11; 32]),
                    ("funder_hash", vec![0x33; 32]),
                    ("pledge", snum(100_000_000)),
                    ("period", snum(1000)),
                ],
            ),
            (
                "SilverScript · Escrow",
                ESCROW_A,
                &[("arbiter_hash", vec![0x33; 32]), ("buyer", vec![0x11; 32]), ("seller", vec![0x22; 32])],
            ),
            (
                "SilverScript · LastWill",
                LASTWILL_A,
                &[("inheritor_hash", vec![0x33; 32]), ("cold_hash", vec![0x44; 32]), ("hot_hash", vec![0x11; 32])],
            ),
        ];
        let skels = silverscript_skeletons();
        for (name, dump, args) in cases {
            let skel = skels.iter().find(|s| s.name == *name).expect("skeleton");
            let args_ref: Vec<(&str, &[u8])> = args.iter().map(|(l, v)| (*l, v.as_slice())).collect();
            let emitted = skel.emit(&args_ref).expect("emit");
            assert_eq!(hex::encode(&emitted), *dump, "{name} emit != dump");
        }
    }

    #[test]
    fn emit_round_trips_fresh_args_including_small_int_selector() {
        let reg = Registry::default();
        let skels = silverscript_skeletons();
        let mecenas = skels.iter().find(|s| s.name == "SilverScript · Mecenas").unwrap();
        // fresh args, pledge with a sign-guard byte (180 -> b4 00), period a small int (6 -> Op6)
        let recipient = vec![0xab; 32];
        let funder = vec![0xcd; 32];
        let pledge = snum(180);
        let period = snum(6);
        let args: Vec<(&str, &[u8])> = vec![
            ("recipient", &recipient),
            ("funder_hash", &funder),
            ("pledge", &pledge),
            ("period", &period),
        ];
        let emitted = mecenas.emit(&args).expect("emit");
        let d = reg.decode(0, &emitted);
        assert_eq!(d.template, Some("SilverScript · Mecenas"));
        let get = |n: &str| d.fields.iter().find(|f| f.name == n).map(|f| f.value.clone());
        assert_eq!(get("recipient"), Some(recipient));
        assert_eq!(get("funder_hash"), Some(funder));
        assert_eq!(get("pledge"), Some(snum(180)));
        assert_eq!(get("period"), Some(snum(6)));
        // period=6 must encode to Op6 (0x56), the canonical small-int push
        assert_eq!(encode_push(&snum(6)), vec![0x56]);
        // and a 32-byte value uses a direct length-prefixed push
        assert_eq!(encode_push(&[0xab; 32])[0], 0x20);
    }

    #[test]
    fn zk_system_classifier() {
        // No ZK op → nothing to say.
        let mut plain = vec![0x20];
        plain.extend([0x00; 32]);
        plain.push(0xac);
        assert_eq!(zk_system(&plain), None);
        assert_eq!(Registry::default().decode(0, &plain).zk_system, None);

        // A ~192-byte proof push (Groth16 band) then OpZkPrecompile → groth16.
        let mut groth = encode_push(&vec![0x01; 192]);
        groth.push(0xa6);
        assert_eq!(zk_system(&groth), Some("groth16"));
        assert!(Registry::default().decode(0, &groth).uses_zk_ops);
        assert_eq!(Registry::default().decode(0, &groth).zk_system, Some("groth16"));

        // A 2 KiB seal push → STARK scale → risc0 (wins over any small push).
        let mut risc0 = encode_push(&vec![0xab; 2048]);
        risc0.extend(encode_push(&vec![0xcd; 32])); // image id
        risc0.push(0xa6);
        assert_eq!(zk_system(&risc0), Some("risc0"));

        // ZK op present but only generic 32-byte pushes → too ambiguous.
        let mut ambiguous = encode_push(&vec![0x07; 32]);
        ambiguous.push(0xa6);
        assert_eq!(zk_system(&ambiguous), None);
    }

    #[test]
    fn observed_skeletons_all_derive() {
        let names: Vec<_> = observed::observed_skeletons().iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            [
                "PURE",
                "genesis0 · list",
                "genesis0 · buy",
                "genesis0 · list",
                "genesis0 · buy",
                "genesis0 · collection",
                "KCC20 token",
                "KCC20 token",
                "KCC20 token",
                "KCC20 minter",
            ],
            "every on-chain fixture pair must derive a skeleton"
        );
        let repeats: Vec<_> = observed::observed_repeat_skeletons().iter().map(|s| s.name).collect();
        assert_eq!(repeats, ["genesis0 · slot-mint"]);
    }

    #[test]
    fn observed_families_match_their_fixture_programs() {
        let reg = Registry::default();
        let get = |d: &Decoded, n: &str| {
            d.fields.iter().find(|f| f.name == n).map(|f| f.value.clone())
        };

        // PURE: the one inlined argument is the CheckSigFromStack key.
        let pure = include_bytes!("../fixtures/pure_a.bin");
        let d = reg.decode(0, pure);
        assert_eq!(d.template, Some("PURE"));
        assert_eq!(
            get(&d, "signer_pubkey").map(hex::encode).as_deref(),
            Some("4df3c68074217004ad86fca1e63b91b73e625d9140063f21992231fdfdfa8936")
        );

        // KCC20 token: state fields land on the contract's labels. Fixture
        // kcc20_b_a is a mint-capable instance owned by a covenant id.
        let d = reg.decode(0, include_bytes!("../fixtures/kcc20_b_a.bin"));
        assert_eq!(d.template, Some("KCC20 token"));
        assert_eq!(get(&d, "owner_identifier").map(|v| v.len()), Some(32));
        assert_eq!(get(&d, "identifier_type"), Some(vec![0x02]));
        assert_eq!(get(&d, "amount"), Some(vec![0; 8]));
        assert_eq!(get(&d, "is_minter"), Some(vec![0x01]));
        // …and kcc20_a_a is a plain pubkey-owned, non-minting instance.
        let d = reg.decode(0, include_bytes!("../fixtures/kcc20_a_a.bin"));
        assert_eq!(d.template, Some("KCC20 token"));
        assert_eq!(get(&d, "identifier_type"), Some(vec![0x00]));
        assert_eq!(get(&d, "amount").map(hex::encode).as_deref(), Some("a00f000000000000"));
        assert_eq!(get(&d, "is_minter"), Some(vec![0x00]));

        // KCC20 minter: the input-side and output-side covenant-id pins fold
        // into one slot per governed token, so exactly two id fields.
        let d = reg.decode(0, include_bytes!("../fixtures/kcc20_minter_a.bin"));
        assert_eq!(d.template, Some("KCC20 minter"));
        assert_eq!(d.fields.len(), 2);
        assert!(d.fields.iter().all(|f| f.value.len() == 32));

        // Marketplace stages + collection registry.
        for (fixture, want) in [
            (include_bytes!("../fixtures/g0_list_v1_a.bin").as_slice(), "genesis0 · list"),
            (include_bytes!("../fixtures/g0_buy_v1_a.bin").as_slice(), "genesis0 · buy"),
            (include_bytes!("../fixtures/g0_list_v2_b.bin").as_slice(), "genesis0 · list"),
            (include_bytes!("../fixtures/g0_buy_v2_b.bin").as_slice(), "genesis0 · buy"),
            (include_bytes!("../fixtures/g0_col_a.bin").as_slice(), "genesis0 · collection"),
        ] {
            assert_eq!(reg.decode(0, fixture).template, Some(want), "fixture for {want}");
        }
        // The list program embeds the follow-up buy state's template bytes.
        let d = reg.decode(0, include_bytes!("../fixtures/g0_list_v1_a.bin"));
        let tmpl = get(&d, "next_state_template").expect("next_state_template");
        assert_eq!(tmpl.len(), 396);
    }

    #[test]
    fn slot_mint_repeat_matcher_covers_all_arities() {
        let reg = Registry::default();
        let get_all = |d: &Decoded, n: &str| {
            d.fields.iter().filter(|f| f.name == n).map(|f| f.value.clone()).collect::<Vec<_>>()
        };

        // DI4M2 build: two per-collection output checks.
        let di4m = include_bytes!("../fixtures/slot_mint_di4m_a.bin");
        let d = reg.decode(0, di4m);
        assert_eq!(d.template, Some("genesis0 · slot-mint"));
        assert_eq!(get_all(&d, "lane_tag"), vec![b"DI4M2".to_vec()]);
        assert_eq!(get_all(&d, "min_outputs"), vec![vec![0x04]]);
        let hashes = get_all(&d, "output_spk_hash");
        assert_eq!(hashes.len(), 2, "two repeats in the DI4M2 arity");
        assert_eq!(
            hex::encode(&hashes[0]),
            "c90a93233366793d3a3576e9677a7e1f31ff85c80ba999fc5ca5dbaeac73a544",
            "first repeat pins the shared marketplace output"
        );

        // GZ4M1 build: one check.
        let d = reg.decode(0, include_bytes!("../fixtures/slot_mint_gz4m_a.bin"));
        assert_eq!(d.template, Some("genesis0 · slot-mint"));
        assert_eq!(get_all(&d, "lane_tag"), vec![b"GZ4M1".to_vec()]);
        assert_eq!(get_all(&d, "min_outputs"), vec![vec![0x03]]);
        assert_eq!(get_all(&d, "output_spk_hash").len(), 1);

        // An unseen third arity still matches: splice in another copy of the
        // repeated block (instructions 102..111 of the DI4M2 build).
        let (insts, _) = disassemble(di4m);
        let start = insts[102].offset;
        let end = insts[111].offset;
        let mut three = di4m[..end].to_vec();
        three.extend_from_slice(&di4m[start..end]);
        three.extend_from_slice(&di4m[end..]);
        let d = reg.decode(0, &three);
        assert_eq!(d.template, Some("genesis0 · slot-mint"), "extra repeat must still match");
        assert_eq!(get_all(&d, "output_spk_hash").len(), 3);

        // A partial copy breaks group divisibility → no template.
        let mut ragged = di4m[..end].to_vec();
        ragged.extend_from_slice(&di4m[start..start + 1]); // lone output-index push
        ragged.extend_from_slice(&di4m[end..]);
        assert_eq!(reg.decode(0, &ragged).template, None);
    }

    #[test]
    fn derive_observed_edge_cases() {
        let a = include_bytes!("../fixtures/pure_a.bin").as_slice();
        let b = include_bytes!("../fixtures/pure_b.bin").as_slice();
        // one instance is not a derivation
        assert!(Skeleton::derive_observed("x", &[a], &["k"]).is_none());
        // label count must equal the distinct slots (one here)
        assert!(Skeleton::derive_observed("x", &[a, b], &[]).is_none());
        assert!(Skeleton::derive_observed("x", &[a, b], &["k", "extra"]).is_none());
        assert!(Skeleton::derive_observed("x", &[a, b], &["k"]).is_some());
        // shape mismatch across instances → None
        let other = include_bytes!("../fixtures/g0_col_a.bin").as_slice();
        assert!(Skeleton::derive_observed("x", &[a, other], &["k"]).is_none());
        // repeat derivation needs both arities
        assert!(RepeatSkeleton::derive("x", &[a, b], &[], &["k"], &[]).is_none());
    }

    #[test]
    fn zk_band_boundaries() {
        let probe = |n: usize| {
            let mut s = encode_push(&vec![0x5a; n]);
            s.push(0xa6); // OpZkPrecompile
            zk_system(&s)
        };
        assert_eq!(probe(127), None);
        assert_eq!(probe(128), Some("groth16"));
        assert_eq!(probe(256), Some("groth16"));
        assert_eq!(probe(257), None, "gap above the Groth16 band stays unattributed");
        assert_eq!(probe(299), None);
        assert_eq!(probe(300), Some("succinct proof (inferred)"));
        assert_eq!(probe(1023), Some("succinct proof (inferred)"));
        assert_eq!(probe(1024), Some("risc0"));
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
