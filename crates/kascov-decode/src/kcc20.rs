//! KCC20 state-level helpers: typed access to the "KCC20 token" state fields
//! and the splice-and-hash primitive that proves an output's hidden state.
//!
//! Every registered KCC20 token build opens with the same alt-stack-guarded
//! state block at fixed byte offsets:
//!
//! ```text
//! 0x6b · 0x20 owner[2..34] · 0x01 type[35] · 0x08 amount[37..45] · 0x01 isMinter[46] · 0x6c
//! ```
//!
//! verified across all 2,561 hash-verified TN10 reveals (state block ok=2561
//! bad=0). Splicing a candidate state into a same-build program and checking
//! blake2b-256(program) against a P2SH commitment is therefore a *proof* of
//! that output's state — hash equality is the sole acceptance criterion, so a
//! misparse can only fail closed, never accept a wrong state. Any future
//! build with different offsets simply never passes the hash check.

use crate::{p2sh_hash, Registry};
use std::ops::Range;

/// Registry template name of the token contract (kcc20.sil).
pub const TOKEN_TEMPLATE: &str = "KCC20 token";
/// Registry template name of the two-token vault build ("minter" is the
/// historical skeleton name; on TN10 these are stateless two-token vaults).
pub const MINTER_TEMPLATE: &str = "KCC20 minter";

/// One decoded KCC20 token state, raw field bytes preserved: hash proofs
/// operate on exact bytes, and amount VALIDITY (script-number range) is a
/// separate judgement from state IDENTITY.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenState {
    pub owner: [u8; 32],
    pub identifier_type: u8,
    /// The raw amount push (observed: always 8-byte little-endian).
    pub amount_raw: Vec<u8>,
    /// The raw isMinter push (observed: always 1 byte, 0x00 / 0x01).
    pub minter_raw: Vec<u8>,
}

impl TokenState {
    /// The amount as a non-negative i64, only for the canonical encoding the
    /// chain uses: exactly 8 LE bytes with the script-number sign bit clear.
    /// Anything else is out of model — callers must treat `None` as
    /// unvalidatable, never coerce.
    pub fn amount_i64(&self) -> Option<i64> {
        let bytes: [u8; 8] = self.amount_raw.as_slice().try_into().ok()?;
        let v = i64::from_le_bytes(bytes);
        (v >= 0).then_some(v)
    }

    /// Whether this cell mints, where a build WITHOUT a minter field answers
    /// "no" rather than "unknown". Absence is not ambiguity: a program with no
    /// such field cannot express minting, so no cell of it can be a minter.
    /// Kept separate from [`Self::is_minter`], which stays strict about what
    /// the bytes literally say.
    pub fn mints(&self) -> Option<bool> {
        if self.minter_raw.is_empty() {
            return Some(false);
        }
        self.is_minter()
    }

    /// Strict boolean read of isMinter; `None` for any non-0x00/0x01 byte.
    pub fn is_minter(&self) -> Option<bool> {
        match self.minter_raw.as_slice() {
            [0x00] => Some(false),
            [0x01] => Some(true),
            _ => None,
        }
    }

    /// Owner key for aggregation: hex(identifier_type || owner_identifier).
    pub fn owner_key(&self) -> String {
        let mut bytes = Vec::with_capacity(33);
        bytes.push(self.identifier_type);
        bytes.extend_from_slice(&self.owner);
        hex::encode(bytes)
    }
}

/// The template a revealed program should be filed under. The registry
/// answers first, since a matched skeleton also labels the program's fields.
/// Where it has no skeleton, a located state block is still evidence enough
/// that the program is a KCC20 token: the block's shape is what the accounting
/// reads, and every value it yields is hash-gated before anything trusts it.
///
/// The fallback is load-bearing, not cosmetic. Skeletons derive from observed
/// fixture PAIRS, so a build can only be registered once the chain has shown
/// each state field varying — and `is_minter` has never varied on the
/// unguarded build KRON deploys. Without this, such a build stays unrecognized
/// at reveal time and its tokens are invisible until a backfill pass reruns.
pub fn revealed_template(
    registry: &Registry,
    spk_version: u16,
    program: &[u8],
) -> Option<&'static str> {
    registry
        .decode(spk_version, program)
        .template
        .or_else(|| locate_state_block(program).map(|_| TOKEN_TEMPLATE))
        // A pinned KCC-0020 build: recognised only when a committed pin's
        // template hash reproduces at its declared cut — an unpinned program
        // is never claimed, however spec-shaped its bytes look. The name is
        // the pin's own family where it declares one, so a venue's cell
        // reads under the venue's prefix, and the generic spec label
        // otherwise.
        .or_else(|| {
            crate::kcc0020::pinned_program_state(program)
                .map(|(pin, _)| pin.family_label())
        })
}

/// A pinned family whose labelled fields carry the KCC20 four-field state.
///
/// Every launchpad compiles its own build, so the state a token index needs
/// turns up under a different template name and sometimes a different field
/// width each time. This table is the one place that mapping lives: adding a
/// row is what puts a launchpad's tokens in the index, with no other wiring.
/// A family belongs here only when its own decode has been proven to carry an
/// owner, an owner-scheme byte and a per-cell amount; anything else stays out
/// and its covenants keep their name without entering the accounting.
pub struct CellFamily {
    /// Template name as the registry reports it.
    pub template: &'static str,
    /// Field label holding the 32-byte owner identifier.
    pub owner: &'static str,
    /// Field label holding the owner-scheme byte. May be wider than one byte
    /// when the build pads it; the high bytes must then all be zero.
    pub identifier_type: &'static str,
    /// Field label holding the little-endian per-cell amount.
    pub amount: &'static str,
    /// Field label holding the minter flag, where the build has one. Families
    /// that carry a token id instead leave this `None`, and their states
    /// decode with an empty `minter_raw`, which `is_minter()` already reads as
    /// "not a strict boolean" rather than as false.
    pub is_minter: Option<&'static str>,
    /// Field label holding a per-cell mode word that the venue's own spend
    /// paths rewrite alongside the balance, where the build has one. KaspaCom
    /// keeps `mint_mode` here: the mintable remainder of a public mint
    /// carries 2 and every cell a mint pays out carries 0, so a base taken
    /// from the remainder reaches the paid-out cells only with this slot
    /// zeroed. It is not part of [`TokenState`] and the accounting never
    /// reads it; it enters a proof only through [`canonical_cell_bases`],
    /// where each variant is hash-gated like every other byte.
    pub mode: Option<&'static str>,
}

/// The registered cell families, most specific first.
pub const CELL_FAMILIES: &[CellFamily] = &[
    CellFamily {
        template: TOKEN_TEMPLATE,
        owner: "owner_identifier",
        identifier_type: "identifier_type",
        amount: "amount",
        is_minter: Some("is_minter"),
        mode: None,
    },
    // KaspaCom's build carries the four fields alongside the token's own
    // identity (ticker, name, decimals) in the same program, and pads the
    // owner-scheme byte to eight bytes. Both its generations (8,076 B and
    // 2,671 B) label mint_mode as their fourth slot, right after amount,
    // and a mint flips it from 2 to 0 on the cells it pays out.
    CellFamily {
        template: "KaspaCom · token",
        owner: "owner_identifier",
        identifier_type: "identifier_type",
        amount: "amount",
        is_minter: None,
        mode: Some("mint_mode"),
    },
    // Zealous cells end in a 32-byte token id where the base build has a
    // one-byte minter flag, which is why they cannot simply be named
    // "KCC20 token": see the note on their skeletons in `observed`.
    CellFamily {
        template: "Zealous · token cell",
        owner: "owner_identifier",
        identifier_type: "identifier_type",
        amount: "amount",
        is_minter: None,
        mode: None,
    },
    // KForge's cell carries the base four fields and nothing else; it is its
    // own family only so the venue owns the name (and with it the venue
    // search and page), never because the layout differs.
    CellFamily {
        template: "KForge · token cell",
        owner: "owner_identifier",
        identifier_type: "identifier_type",
        amount: "amount",
        is_minter: Some("is_minter"),
        mode: None,
    },
];

/// Every template name that carries token state, for callers that need to
/// select candidates before decoding (the store's derivation does this in
/// SQL): the legacy [`CELL_FAMILIES`] names, then every name a pinned
/// KCC-0020 reveal can be stamped with (the generic spec label and each
/// pin's family, see `kcc0020::spec_template_names`). Kept in sync with
/// both tables by construction, and [`is_cell_template`] is the same set as
/// a predicate.
pub fn cell_template_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = CELL_FAMILIES.iter().map(|f| f.template).collect();
    names.extend(crate::kcc0020::spec_template_names());
    names
}

/// Whether a template name is one of [`cell_template_names`], without
/// building the list: the store's per-UTXO recognition hook asks this for
/// every row it writes.
pub fn is_cell_template(name: &str) -> bool {
    CELL_FAMILIES.iter().any(|f| f.template == name) || crate::kcc0020::is_spec_template(name)
}

/// Read the owner-scheme byte from a field that may be padded wider than one
/// byte. Returns `None` when any byte above the first is set, so a value that
/// does not actually fit a scheme byte is refused rather than truncated.
fn scheme_byte(raw: &[u8]) -> Option<u8> {
    let (first, rest) = raw.split_first()?;
    rest.iter().all(|b| *b == 0).then_some(*first)
}

/// Decode `program` as a KCC20 token state via the registry skeletons.
/// Matches any family in [`CELL_FAMILIES`], requiring a 32-byte owner and an
/// owner-scheme byte that fits in one byte — a partial or misshapen decode
/// yields `None` and falls through to the structural reader.
pub fn decode_token_state(
    registry: &Registry,
    spk_version: u16,
    program: &[u8],
) -> Option<TokenState> {
    let d = registry.decode(spk_version, program);
    if let Some(fam) = d
        .template
        .and_then(|t| CELL_FAMILIES.iter().find(|f| f.template == t))
    {
        let field = |name: &str| {
            d.fields
                .iter()
                .find(|f| f.name == name)
                .map(|f| f.value.clone())
        };
        let owner: [u8; 32] = field(fam.owner)?.try_into().ok()?;
        let identifier_type = scheme_byte(&field(fam.identifier_type)?)?;
        return Some(TokenState {
            owner,
            identifier_type,
            amount_raw: field(fam.amount)?,
            // A family without a minter flag decodes with an empty
            // `minter_raw` rather than a fabricated 0: is_minter() already
            // treats anything that is not 0x00/0x01 as "no strict answer".
            minter_raw: match fam.is_minter {
                Some(label) => field(label)?,
                None => Vec::new(),
            },
        });
    }
    // No registered skeleton matched. A skeleton is one WAY to recognize a
    // token, not the definition of one: a build the fixtures never captured
    // still carries the same state block, and requiring the pinned name is
    // exactly what hid a live mainnet token behind "p2sh commitment". Read the
    // fields from the located block instead. This is a CANDIDATE only, and
    // callers that touch supply must put it through `prove_output_state`,
    // which fails closed unless the spliced program hashes to the on-chain
    // commitment. That makes this path strictly harder to fool than a
    // shape-only skeleton match, not easier.
    decode_state_block(program)
}

/// Read the four state fields straight from a located block, with no registry
/// involved. Unproven on its own: see [`prove_output_state`].
pub fn decode_state_block(program: &[u8]) -> Option<TokenState> {
    let b = locate_state_block(program)?;
    Some(TokenState {
        owner: program.get(b.owner())?.try_into().ok()?,
        identifier_type: *program.get(b.identifier_type())?,
        amount_raw: program.get(b.amount())?.to_vec(),
        minter_raw: vec![*program.get(b.is_minter())?],
    })
}

/// Where a KCC20 state block sits in a program, and how it is framed.
///
/// The four state fields always appear in the same order with the same widths;
/// builds differ only in whether the block is wrapped in the alt-stack guards
/// (`OpToAltStack` / `OpFromAltStack`). One `start` offset therefore describes
/// every build seen on chain, and every accessor below is derived from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateBlock {
    /// Offset of the push32 opcode that opens the block.
    pub start: usize,
    /// True when the block is wrapped in alt-stack guards. Guarded and
    /// unguarded builds are distinct templates and must not be conflated: the
    /// KCC-1 template hash covers the surrounding bytes, so the two hash apart.
    pub guarded: bool,
}

impl StateBlock {
    /// owner_identifier, 32 bytes.
    pub fn owner(&self) -> std::ops::Range<usize> {
        self.start + 1..self.start + 33
    }
    /// identifier_type, 1 byte.
    pub fn identifier_type(&self) -> usize {
        self.start + 34
    }
    /// amount, 8 bytes little-endian.
    pub fn amount(&self) -> std::ops::Range<usize> {
        self.start + 36..self.start + 44
    }
    /// is_minter, 1 byte.
    pub fn is_minter(&self) -> usize {
        self.start + 45
    }
    /// One past the last byte of the block.
    pub fn end(&self) -> usize {
        self.start + 46
    }
}

/// Locate the KCC20 state block in `program`, accepting every build observed
/// on chain rather than one pinned compiled shape.
///
/// kascov originally required the guarded build byte-for-byte at fixed offsets.
/// Mainnet carries a second build that emits the identical field layout with no
/// alt-stack guards, shifted by exactly one byte; under the old predicate all
/// 1,888 programs of a live token failed every check and the token fell through
/// to the generic "p2sh commitment" bucket. Identity is located here, then
/// PROVEN by hash in [`prove_output_state`] before any accounting trusts it.
pub fn locate_state_block(program: &[u8]) -> Option<StateBlock> {
    // Guarded build: 0x6b … 0x6c wrapping the block at offset 1.
    if program.len() >= 48
        && program[0] == 0x6b
        && program[1] == 0x20
        && program[34] == 0x01
        && program[36] == 0x08
        && program[45] == 0x01
        && program[47] == 0x6c
    {
        return Some(StateBlock {
            start: 1,
            guarded: true,
        });
    }
    // Unguarded build: the same pushes with no guards, block at offset 0.
    if program.len() >= 46
        && program[0] == 0x20
        && program[33] == 0x01
        && program[35] == 0x08
        && program[44] == 0x01
    {
        return Some(StateBlock {
            start: 0,
            guarded: false,
        });
    }
    None
}

/// Does `program` carry a KCC20 state block in any known build?
pub fn has_state_block(program: &[u8]) -> bool {
    locate_state_block(program).is_some()
}

/// The KCC-1 template parts of a program carrying the verified KCC20 state
/// block: prefix is everything before the block (the leading alt-stack guard
/// byte, or nothing), suffix is everything from the closing guard on. `None`
/// when the block is absent — the cut is only known where the state range is
/// proven, never guessed.
pub fn kcc1_template_parts(program: &[u8]) -> Option<(&[u8], &[u8])> {
    let b = locate_state_block(program)?;
    Some((&program[..b.start], &program[b.end()..]))
}

/// KCC-1 §8.3 TemplateHash (BLAKE3, merged draft at ea5176aa) of a program
/// carrying the verified KCC20 state block; `None` when the block is absent.
/// Recompute after a spec change is gated by the store's `kcc1_abi_version`
/// meta, which also records the previous name as an alias.
pub fn kcc1_template_hash(program: &[u8]) -> Option<[u8; 32]> {
    let (prefix, suffix) = kcc1_template_parts(program)?;
    Some(crate::kcc1::template_hash(prefix, suffix))
}

/// The same template under the pre-merge draft's BLAKE2b naming (kccs
/// 55b28d8): the number kascov published for this family before the re-pin,
/// kept only so old links resolve. Never a canonical identity.
pub fn kcc1_template_hash_55b28d8(program: &[u8]) -> Option<[u8; 32]> {
    let (prefix, suffix) = kcc1_template_parts(program)?;
    Some(crate::argent::template_hash(prefix, suffix))
}

/// Splice a candidate state into a same-build program at the fixed state
/// block. Returns `None` when the base program doesn't carry the block.
/// The result is only meaningful after a hash check against a commitment.
pub fn splice_token_state(
    program: &[u8],
    owner: &[u8; 32],
    identifier_type: u8,
    amount: &[u8; 8],
    is_minter: u8,
) -> Option<Vec<u8>> {
    let b = locate_state_block(program)?;
    let mut p = program.to_vec();
    p[b.owner()].copy_from_slice(owner);
    p[b.identifier_type()] = identifier_type;
    p[b.amount()].copy_from_slice(amount);
    p[b.is_minter()] = is_minter;
    Some(p)
}

/// The slot ranges of `program` under the one skeleton of `template` that
/// fits it. A family name covers several builds (five Zealous token cells,
/// two KaspaCom generations) and a skeleton answers only for its own build,
/// so the skeletons carrying the name are tried in registration order and
/// the first whose `slot_ranges` accepts the program wins. Taking the first
/// skeleton by name instead refused every parent of a later build, which is
/// what left the 1,811 B and 1,363 B Zealous cells without a splice base.
fn family_slot_ranges(
    template: &str,
    program: &[u8],
) -> Option<Vec<(&'static str, Range<usize>)>> {
    cell_skeletons()
        .iter()
        .filter(|s| s.name == template)
        .find_map(|s| s.slot_ranges(program))
}

/// A registered family's layout in one program: the family row and the byte
/// range of every labelled slot, under the skeleton that fits this build.
/// `None` when the registry does not name the program as a cell family or
/// no skeleton of that family accepts it.
fn family_layout(
    registry: &Registry,
    program: &[u8],
) -> Option<(&'static CellFamily, Vec<(&'static str, Range<usize>)>)> {
    let template = registry.decode(0, program).template?;
    let fam = CELL_FAMILIES.iter().find(|f| f.template == template)?;
    let ranges = family_slot_ranges(template, program)?;
    Some((fam, ranges))
}

/// Write `src` into `range` of `out`, little-endian at the slot's own width:
/// a shorter value is zero-extended and one that does not fit is refused
/// rather than truncated. Every splice path writes through here, so the
/// width rule holds in one place.
fn write_slot(out: &mut [u8], range: &Range<usize>, src: &[u8]) -> bool {
    let width = range.len();
    if src.len() > width && src[width..].iter().any(|b| *b != 0) {
        return false;
    }
    let n = src.len().min(width);
    out[range.start..range.start + n].copy_from_slice(&src[..n]);
    for b in &mut out[range.start + n..range.end] {
        *b = 0;
    }
    true
}

/// Splice a candidate state into a program of ANY registered cell family,
/// using that family's own slot positions rather than the base build's fixed
/// offsets. Widths are preserved exactly: a build that pads its owner-scheme
/// byte to eight bytes gets eight bytes back, little-endian, and a value that
/// does not fit its slot is refused rather than truncated. `mode` is written
/// into the family's mode slot where it labels one (`Some(0)` zeroes it at
/// its own width); `None` carries the slot exactly as the program has it.
///
/// Like the base splice, the result means nothing until it is hash-checked.
fn splice_family_state(
    registry: &Registry,
    program: &[u8],
    owner: &[u8; 32],
    identifier_type: u8,
    amount: &[u8; 8],
    mode: Option<u64>,
) -> Option<Vec<u8>> {
    let (fam, ranges) = family_layout(registry, program)?;
    splice_into(fam, &ranges, program, owner, identifier_type, amount, mode)
}

/// The write half of [`splice_family_state`], for callers that already hold
/// the layout and splice the same program more than once.
fn splice_into(
    fam: &CellFamily,
    ranges: &[(&'static str, Range<usize>)],
    program: &[u8],
    owner: &[u8; 32],
    identifier_type: u8,
    amount: &[u8; 8],
    mode: Option<u64>,
) -> Option<Vec<u8>> {
    let mode_bytes = mode.map(u64::to_le_bytes);
    let mut p = program.to_vec();
    let mut wrote_owner = false;
    let mut wrote_amount = false;
    for (label, range) in ranges {
        let src: &[u8] = if *label == fam.owner {
            wrote_owner = true;
            owner
        } else if *label == fam.identifier_type {
            &[identifier_type]
        } else if *label == fam.amount {
            wrote_amount = true;
            amount
        } else if fam.mode == Some(*label) {
            match &mode_bytes {
                Some(m) => m,
                None => continue,
            }
        } else {
            continue;
        };
        if !write_slot(&mut p, range, src) {
            return None;
        }
    }
    (wrote_owner && wrote_amount).then_some(p)
}

/// A prepared splice plan for one base program.
///
/// Recovery tries many candidate states against ONE base, and locating the
/// state costs a full disassembly of the program. Doing that per candidate is
/// what kept the pass hitting its wall-clock budget: the work is identical
/// every time, because a base's layout does not depend on the state spliced
/// into it. Build this once per base, then each attempt is a few byte writes
/// and a hash.
///
/// A family's mode slot is not an axis of the plan: [`canonical_cell_bases`]
/// realises it as separate bases, so one plan always writes exactly the
/// three state fields and, where the build has one, the minter flag.
pub struct CellSplicer {
    base: Vec<u8>,
    owner: Range<usize>,
    identifier_type: Range<usize>,
    amount: Range<usize>,
    is_minter: Option<Range<usize>>,
}

impl CellSplicer {
    /// Prepare a plan, or `None` when this program is not a spliceable cell.
    /// A family's skeletons are tried per build, so a base of any registered
    /// generation prepares, not only the one registered first under the name.
    pub fn new(registry: &Registry, base: &[u8]) -> Option<Self> {
        if let Some(b) = locate_state_block(base) {
            return Some(Self {
                base: base.to_vec(),
                owner: b.owner(),
                identifier_type: b.identifier_type()..b.identifier_type() + 1,
                amount: b.amount(),
                is_minter: Some(b.is_minter()..b.is_minter() + 1),
            });
        }
        let (fam, ranges) = family_layout(registry, base)?;
        let pick = |label: &str| {
            ranges
                .iter()
                .find(|(l, _)| *l == label)
                .map(|(_, r)| r.clone())
        };
        Some(Self {
            base: base.to_vec(),
            owner: pick(fam.owner)?,
            identifier_type: pick(fam.identifier_type)?,
            amount: pick(fam.amount)?,
            is_minter: fam.is_minter.and_then(pick),
        })
    }

    /// Whether this base carries a minter slot at all. A family without one
    /// (KaspaCom, Zealous) decodes every state with an empty `minter_raw`,
    /// and [`Self::prove`] ignores its `is_minter` argument on such a base,
    /// so every minter value a caller tries there splices the same bytes and
    /// repeats the same hash. A candidate search should walk that axis only
    /// when this answers true.
    pub fn has_minter_slot(&self) -> bool {
        self.is_minter.is_some()
    }

    /// Splice a candidate and accept it only if the result hashes to the
    /// output's own commitment. The hash check is the whole guarantee: a
    /// wrong guess costs one hash and can never be accepted.
    pub fn prove(
        &self,
        output_spk: &[u8],
        owner: &[u8; 32],
        identifier_type: u8,
        amount: &[u8; 8],
        is_minter: u8,
    ) -> Option<TokenState> {
        let want = p2sh_hash(output_spk)?;
        let mut p = self.base.clone();
        if !write_slot(&mut p, &self.owner, owner)
            || !write_slot(&mut p, &self.identifier_type, &[identifier_type])
            || !write_slot(&mut p, &self.amount, amount)
        {
            return None;
        }
        if let Some(r) = &self.is_minter {
            if !write_slot(&mut p, r, &[is_minter]) {
                return None;
            }
        }
        (blake2b_256(&p) == want).then(|| TokenState {
            owner: *owner,
            identifier_type,
            amount_raw: amount.to_vec(),
            minter_raw: match &self.is_minter {
                Some(_) => vec![is_minter],
                None => Vec::new(),
            },
        })
    }

    /// The spliced program for an already-proven state.
    pub fn program_for(
        &self,
        owner: &[u8; 32],
        identifier_type: u8,
        amount: &[u8; 8],
        is_minter: u8,
    ) -> Option<Vec<u8>> {
        let mut p = self.base.clone();
        write_slot(&mut p, &self.owner, owner).then_some(())?;
        write_slot(&mut p, &self.identifier_type, &[identifier_type]).then_some(())?;
        write_slot(&mut p, &self.amount, amount).then_some(())?;
        if let Some(r) = &self.is_minter {
            write_slot(&mut p, r, &[is_minter]).then_some(())?;
        }
        Some(p)
    }
}

/// Process-wide skeleton list, derived once.
///
/// Deriving them is expensive: it disassembles two fixtures per family, and
/// some are very large. Recovery calls the splice once per candidate hash, so
/// rebuilding the list there put a full re-derivation of every family inside
/// the innermost loop and starved the pass's wall-clock budget long before its
/// hash budget. Same reasoning, and same fix, as the decode registry.
fn cell_skeletons() -> &'static [crate::Skeleton] {
    static SKELETONS: std::sync::OnceLock<Vec<crate::Skeleton>> = std::sync::OnceLock::new();
    SKELETONS.get_or_init(|| {
        crate::observed::observed_skeletons()
            .into_iter()
            .chain(crate::x402_skeletons())
            .collect()
    })
}

/// Canonicalise a cell program into its splice BASES: the same program with
/// its per-cell state zeroed, so every cell of one build reduces to one base
/// instead of multiplying the candidate search. Works for the base build and
/// for any registered family.
///
/// The first entry keeps every other slot exactly as the program carries it.
/// Where the family labels a mode slot and the program's mode is non-zero, a
/// second entry carries the same base with that slot zeroed: a KaspaCom mint
/// pays out cells at mint_mode 0 from a remainder at 2, so a base taken from
/// the parent reaches those cells only in this variant, while the remainder
/// itself is reached by the first. A program already at mode 0 yields one
/// entry, since the variant would repeat it. Each entry is still only a
/// base: nothing is accepted until a splice into it hashes to a live cell's
/// own commitment.
///
/// Empty when the program is not a recognised cell at all, which the caller
/// must treat as "no usable base" rather than as an error: recovery simply
/// has one fewer base to try, and a proof it cannot attempt is a missing
/// balance, never a wrong one.
pub fn canonical_cell_bases(registry: &Registry, program: &[u8]) -> Vec<Vec<u8>> {
    if has_state_block(program) {
        return splice_token_state(program, &[0u8; 32], 0, &[0u8; 8], 0)
            .into_iter()
            .collect();
    }
    let Some((fam, ranges)) = family_layout(registry, program) else {
        return Vec::new();
    };
    let zeroed = |mode: Option<u64>| {
        splice_into(fam, &ranges, program, &[0u8; 32], 0, &[0u8; 8], mode)
    };
    let Some(kept) = zeroed(None) else {
        return Vec::new();
    };
    let mut bases = vec![kept];
    let mode_is_set = ranges.iter().any(|(label, range)| {
        fam.mode == Some(*label)
            && program
                .get(range.clone())
                .is_some_and(|slot| slot.iter().any(|b| *b != 0))
    });
    if mode_is_set {
        bases.extend(zeroed(Some(0)));
    }
    bases
}

/// The as-is entry of [`canonical_cell_bases`], for callers that want one
/// base per program: the base that keeps every non-state slot exactly as
/// the program carries it. `None` when the program is not a recognised cell.
pub fn canonical_cell_base(registry: &Registry, program: &[u8]) -> Option<Vec<u8>> {
    canonical_cell_bases(registry, program).into_iter().next()
}

/// Is this program a cell kascov can splice at all, by either route?
pub fn is_spliceable_cell(registry: &Registry, program: &[u8]) -> bool {
    has_state_block(program)
        || registry
            .decode(0, program)
            .template
            .is_some_and(|t| CELL_FAMILIES.iter().any(|f| f.template == t))
}

/// Prove a P2SH-committed output's state for any registered cell family.
///
/// Unspent cells never reveal their program, so their balance can only be
/// read by rebuilding a candidate and checking it against the commitment the
/// output already carries. Falls back to the base build's fixed-offset splice,
/// so nothing about the original path changes.
pub fn prove_output_state_any(
    registry: &Registry,
    base_program: &[u8],
    output_spk: &[u8],
    owner: &[u8; 32],
    identifier_type: u8,
    amount: &[u8; 8],
    is_minter: u8,
) -> Option<(TokenState, Vec<u8>)> {
    if let Some(st) = prove_output_state(
        base_program,
        output_spk,
        owner,
        identifier_type,
        amount,
        is_minter,
    ) {
        let program = splice_token_state(base_program, owner, identifier_type, amount, is_minter)?;
        return Some((st, program));
    }
    let want = p2sh_hash(output_spk)?;
    let candidate =
        splice_family_state(registry, base_program, owner, identifier_type, amount, None)?;
    (blake2b_256(&candidate) == want).then(|| (TokenState {
        owner: *owner,
        identifier_type,
        amount_raw: amount.to_vec(),
        // families in this path carry no minter flag; absence stays absence
        minter_raw: Vec::new(),
    }, candidate))
}

/// blake2b-256 — the hash Kaspa P2SH commitments use (same parameters as
/// [`crate::p2sh_reveal`]'s verification).
pub fn blake2b_256(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(
        blake2b_simd::Params::new()
            .hash_length(32)
            .hash(bytes)
            .as_bytes(),
    );
    out
}

/// Prove a P2SH-committed output's state: splice the candidate fields into a
/// same-build program and accept iff the spliced program hashes to the
/// output's committed hash. Returns the proven state, or `None` (fails
/// closed on wrong build, wrong candidate, or a non-P2SH spk).
pub fn prove_output_state(
    base_program: &[u8],
    output_spk: &[u8],
    owner: &[u8; 32],
    identifier_type: u8,
    amount: &[u8; 8],
    is_minter: u8,
) -> Option<TokenState> {
    let want = p2sh_hash(output_spk)?;
    let candidate = splice_token_state(base_program, owner, identifier_type, amount, is_minter)?;
    (blake2b_256(&candidate) == want).then(|| TokenState {
        owner: *owner,
        identifier_type,
        amount_raw: amount.to_vec(),
        minter_raw: vec![is_minter],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All three registered builds: real on-chain reveal programs.
    fn builds() -> [&'static [u8]; 3] {
        [
            include_bytes!("../fixtures/kcc20_a_a.bin").as_slice(),
            include_bytes!("../fixtures/kcc20_b_a.bin").as_slice(),
            include_bytes!("../fixtures/kcc20_c_a.bin").as_slice(),
        ]
    }

    /// A real mainnet program from a live token whose build carries NO
    /// alt-stack guards. Under the old fixed-offset predicate every one of the
    /// 1,888 programs in this covenant failed all six checks, so the token was
    /// filed as a generic "p2sh commitment" and never appeared as a token.
    const UNGUARDED_KRON: &[u8] = include_bytes!("../fixtures/kcc20_unguarded_kron.bin");

    #[test]
    fn locates_the_state_block_in_both_builds() {
        // Guarded: the build kascov shipped fixtures for.
        for base in builds() {
            let b = locate_state_block(base).expect("guarded build must locate");
            assert_eq!(
                b,
                StateBlock {
                    start: 1,
                    guarded: true
                }
            );
        }
        // Unguarded: the same field layout with the guards removed, shifted by
        // exactly one byte. This is the case that hid a live mainnet token.
        let b = locate_state_block(UNGUARDED_KRON).expect("unguarded build must locate");
        assert_eq!(
            b,
            StateBlock {
                start: 0,
                guarded: false
            }
        );
        assert_eq!(UNGUARDED_KRON.len(), 2433);
        // The old predicate's very first byte check is what failed.
        assert_ne!(
            UNGUARDED_KRON[0], 0x6b,
            "this fixture exists because it is not guarded"
        );
    }

    #[test]
    fn decodes_the_unguarded_build_without_a_registered_skeleton() {
        let registry = Registry::default();
        // No skeleton matches this build, so the registry cannot name it...
        assert_ne!(
            registry.decode(1, UNGUARDED_KRON).template,
            Some(TOKEN_TEMPLATE)
        );
        // ...yet the state block is present and decodes to the real on-chain
        // values, which is precisely the token kascov used to miss.
        let st = decode_token_state(&registry, 1, UNGUARDED_KRON)
            .expect("an unregistered build must still decode from its located block");
        assert_eq!(
            hex::encode(st.owner),
            "005f70b2d4ca0ff5b9106778a24e5c3551f1e36a61399faadd2a68de592132a0"
        );
        assert_eq!(st.identifier_type, 3);
        assert_eq!(st.amount_i64(), Some(5_000_000));
        assert_eq!(st.is_minter(), Some(false));
    }

    #[test]
    fn unguarded_build_splices_proves_and_hashes_apart_from_guarded() {
        // Splicing must respect the located offsets, not the guarded ones.
        let owner = [0x5au8; 32];
        let amount = 123_456i64.to_le_bytes();
        let spliced = splice_token_state(UNGUARDED_KRON, &owner, 2, &amount, 1).unwrap();
        assert_eq!(spliced.len(), UNGUARDED_KRON.len());
        let st = decode_state_block(&spliced).expect("spliced unguarded program must decode");
        assert_eq!(st.owner, owner);
        assert_eq!(st.identifier_type, 2);
        assert_eq!(st.amount_i64(), Some(123_456));
        assert_eq!(st.is_minter(), Some(true));

        // Everything outside the state block is untouched, so the two builds
        // are distinct templates and must NOT collide on the KCC-1 hash.
        let guarded_hash = kcc1_template_hash(builds()[0]).unwrap();
        let unguarded_hash = kcc1_template_hash(UNGUARDED_KRON).unwrap();
        assert_ne!(guarded_hash, unguarded_hash);
        // The hash is state-independent: re-splicing changes no template bytes.
        assert_eq!(kcc1_template_hash(&spliced).unwrap(), unguarded_hash);
    }

    #[test]
    fn splice_then_decode_roundtrips_on_all_builds() {
        let registry = Registry::default();
        for base in builds() {
            assert!(has_state_block(base));
            let owner = [0xabu8; 32];
            let amount = 71_753i64.to_le_bytes();
            for (id_type, minter) in [(0x00u8, 0x00u8), (0x02, 0x01)] {
                let spliced = splice_token_state(base, &owner, id_type, &amount, minter).unwrap();
                let st = decode_token_state(&registry, 1, &spliced)
                    .expect("spliced program must still decode as KCC20 token");
                assert_eq!(st.owner, owner);
                assert_eq!(st.identifier_type, id_type);
                assert_eq!(st.amount_i64(), Some(71_753));
                assert_eq!(st.is_minter(), Some(minter == 1));
            }
        }
    }

    #[test]
    fn prove_output_state_accepts_only_the_committed_state() {
        let base = builds()[0];
        let owner = [0x11u8; 32];
        let amount = 4_000i64.to_le_bytes();
        let committed = splice_token_state(base, &owner, 0x00, &amount, 0x00).unwrap();
        let mut spk = vec![0xaa, 0x20];
        spk.extend_from_slice(&blake2b_256(&committed));
        spk.push(0x87);

        let st = prove_output_state(base, &spk, &owner, 0x00, &amount, 0x00).unwrap();
        assert_eq!(st.amount_i64(), Some(4_000));
        // A single wrong field byte fails closed.
        assert!(prove_output_state(base, &spk, &owner, 0x02, &amount, 0x00).is_none());
        let wrong_amount = 4_001i64.to_le_bytes();
        assert!(prove_output_state(base, &spk, &owner, 0x00, &wrong_amount, 0x00).is_none());
        // A different build as splice base fails closed too.
        assert!(prove_output_state(builds()[1], &spk, &owner, 0x00, &amount, 0x00).is_none());
    }

    #[test]
    fn amount_strictness() {
        let mk = |raw: &[u8]| TokenState {
            owner: [0; 32],
            identifier_type: 0,
            amount_raw: raw.to_vec(),
            minter_raw: vec![0],
        };
        assert_eq!(mk(&i64::MAX.to_le_bytes()).amount_i64(), Some(i64::MAX));
        assert_eq!(mk(&0i64.to_le_bytes()).amount_i64(), Some(0));
        // Sign bit set = negative script number: out of model, never a u64.
        assert_eq!(mk(&[0, 0, 0, 0, 0, 0, 0, 0x80]).amount_i64(), None);
        // Non-8-byte widths are out of model (chain uses fixed 8-byte LE).
        assert_eq!(mk(&[1, 0, 0, 0]).amount_i64(), None);
        assert_eq!(mk(&[]).amount_i64(), None);
        // isMinter strictness
        let mut st = mk(&1i64.to_le_bytes());
        st.minter_raw = vec![2];
        assert_eq!(st.is_minter(), None);
        st.minter_raw = vec![];
        assert_eq!(st.is_minter(), None);
    }
}

#[cfg(test)]
mod cell_family_tests {
    use super::*;

    /// The point of the family table: a launchpad build that is NOT named
    /// "KCC20 token" still yields the state a token index needs. Without this
    /// the covenant gets a name and stays out of the accounting, which is the
    /// exact gap that kept KaspaCom's tokens off the token pages.
    #[test]
    fn a_kaspacom_cell_decodes_to_token_state() {
        let reg = Registry::default();
        let program = include_bytes!("../fixtures/kcom_token_b.bin").as_slice();

        assert_eq!(
            revealed_template(&reg, 0, program),
            Some("KaspaCom · token"),
            "the family must be recognised before it can be decoded"
        );

        let st = decode_token_state(&reg, 0, program).expect("must decode to token state");
        assert_eq!(st.owner.len(), 32);
        // their build pads the owner-scheme byte to eight bytes; the reader
        // narrows it only when the high bytes are zero
        assert!(matches!(st.identifier_type, 0 | 1 | 2 | 3));
        assert_eq!(
            u64::from_le_bytes(st.amount_raw[..8].try_into().unwrap()),
            90_000_000_000_000_000,
            "amount is the cell's balance, not a supply counter"
        );
        // no minter flag in this build: absent, never fabricated as false
        assert!(st.minter_raw.is_empty());
        assert_eq!(st.is_minter(), None);
    }

    /// A padded scheme byte is narrowed only when nothing is lost. Anything
    /// that does not fit is refused rather than truncated, so a misread build
    /// fails closed instead of inventing an owner scheme.
    #[test]
    fn a_padded_scheme_byte_is_narrowed_only_when_lossless() {
        assert_eq!(scheme_byte(&[2, 0, 0, 0, 0, 0, 0, 0]), Some(2));
        assert_eq!(scheme_byte(&[0]), Some(0));
        assert_eq!(scheme_byte(&[2, 0, 0, 1]), None, "high byte set must refuse");
        assert_eq!(scheme_byte(&[]), None);
    }

    /// Every registered family must name a template the registry can actually
    /// produce, or the row is dead weight that silently does nothing. The
    /// same holds for a labelled mode slot: every skeleton under the family's
    /// name must derive that label, or a generation without it would
    /// silently lose its second base variant and the cells only that variant
    /// reaches.
    #[test]
    fn every_cell_family_names_a_real_template() {
        let skeletons = crate::observed::observed_skeletons();
        let names: Vec<&str> = skeletons.iter().map(|s| s.name).collect();
        for fam in CELL_FAMILIES {
            if fam.template == TOKEN_TEMPLATE {
                continue; // also reachable via the structural fallback
            }
            assert!(
                names.contains(&fam.template),
                "{} is registered as a cell family but no skeleton derives it",
                fam.template
            );
            if let Some(mode) = fam.mode {
                for s in skeletons.iter().filter(|s| s.name == fam.template) {
                    assert!(
                        s.params().contains(&mode),
                        "{} labels a mode slot {mode} that one of its skeletons does not derive",
                        fam.template
                    );
                }
            }
        }
        // KaspaCom is the one family with a mode, and both its generations
        // keep mint_mode as their fourth slot, right after amount.
        let kaspacom = CELL_FAMILIES
            .iter()
            .find(|f| f.template == "KaspaCom · token")
            .expect("KaspaCom is a registered family");
        assert_eq!(kaspacom.mode, Some("mint_mode"));
        let generations: Vec<&crate::Skeleton> = skeletons
            .iter()
            .filter(|s| s.name == kaspacom.template)
            .collect();
        assert_eq!(generations.len(), 2, "the 8,076 B and the 2,671 B builds");
        for s in generations {
            assert_eq!(s.params().get(3), Some(&"mint_mode"));
        }
        for fam in CELL_FAMILIES.iter().filter(|f| f.template != kaspacom.template) {
            assert_eq!(fam.mode, None, "{} labels no mode slot", fam.template);
        }
    }

    /// A pinned KCC-0020 reveal is named by its pin's family where the pin
    /// declares one, and by the generic spec label where it does not, so a
    /// venue's cell reads under the venue's prefix while the reference
    /// vector keeps the spec's own name.
    #[test]
    fn a_pinned_reveal_is_named_by_its_pin_family() {
        let reg = Registry::default();
        let zealous = include_bytes!("../fixtures/kcc0020/zealous_token_a.bin").as_slice();
        assert_eq!(revealed_template(&reg, 0, zealous), Some("Zealous · token"));
        // the next generation reads under the same family, and nothing
        // earlier in the chain of decoders (a skeleton, the 46-byte locator)
        // claims it first
        for later in [
            include_bytes!("../fixtures/kcc0020/zealous_token_2641_a.bin").as_slice(),
            include_bytes!("../fixtures/kcc0020/zealous_token_3037_a.bin").as_slice(),
            include_bytes!("../fixtures/kcc0020/zealous_token_3223_a.bin").as_slice(),
        ] {
            assert_eq!(revealed_template(&reg, 0, later), Some("Zealous · token"), "each later generation reads under the same family");
            assert!(locate_state_block(later).is_none());
        }
        let vector = include_bytes!("../fixtures/kcc0020/vector_template_a.bin").as_slice();
        assert_eq!(
            revealed_template(&reg, 0, vector),
            Some(crate::kcc0020::SPEC_TOKEN_TEMPLATE)
        );
        // the label is display only: the pin still matches either program
        assert!(crate::kcc0020::pinned_program_state(zealous).is_some());
        assert!(crate::kcc0020::pinned_program_state(vector).is_some());
    }

    /// The candidate list every SQL gate is generated from carries the
    /// generic spec label AND every pin family, once each, and the
    /// predicate form agrees with it name for name.
    #[test]
    fn cell_template_names_carry_every_spec_label_once() {
        let names = cell_template_names();
        assert!(names.contains(&crate::kcc0020::SPEC_TOKEN_TEMPLATE));
        assert!(names.contains(&"Zealous · token"));
        for pin in crate::kcc0020::pins() {
            assert!(names.contains(&pin.family_label()), "{} is missing", pin.family_label());
        }
        let mut seen = std::collections::HashSet::new();
        for name in &names {
            assert!(seen.insert(*name), "{name} is listed twice");
            assert!(is_cell_template(name), "{name} is listed but not recognised");
        }
        assert!(!is_cell_template("genesis0 · auction"));
        assert!(!is_cell_template(MINTER_TEMPLATE));
    }
}

#[cfg(test)]
mod family_splice_tests {
    use super::*;

    /// The safety property that makes an unspent cell's balance trustworthy:
    /// splicing a cell's OWN state back into its own program must reproduce
    /// the program byte for byte, and therefore its committed hash.
    ///
    /// If this ever fails, balances for live cells are unreadable (fail
    /// closed). If the hash check were ever dropped, they would be forgeable
    /// — which is why the proof, not the splice, is the load-bearing step.
    #[test]
    fn splicing_a_cells_own_state_reproduces_it_exactly() {
        let reg = Registry::default();
        let program = include_bytes!("../fixtures/kcom_token_b.bin").as_slice();
        let st = decode_token_state(&reg, 0, program).expect("decodes");

        let mut amount = [0u8; 8];
        amount.copy_from_slice(&st.amount_raw[..8]);
        let spliced =
            splice_family_state(&reg, program, &st.owner, st.identifier_type, &amount, None)
                .expect("splices");
        assert_eq!(spliced, program, "own state must round-trip byte for byte");

        // and it must pass the real proof against its own P2SH commitment
        let mut spk = vec![0xaa, 0x20];
        spk.extend_from_slice(&blake2b_256(program));
        spk.push(0x87);
        let (proven, rebuilt) = prove_output_state_any(
            &reg, program, &spk, &st.owner, st.identifier_type, &amount, 0,
        )
        .expect("must prove against its own commitment");
        assert_eq!(proven.owner, st.owner);
        assert_eq!(proven.amount_raw, st.amount_raw);
        assert_eq!(rebuilt, program, "the proven program IS the cell's program");
        // absence of a minter field answers "does not mint", not "unknown"
        assert_eq!(proven.mints(), Some(false));
        assert_eq!(proven.is_minter(), None, "the strict read stays strict");
    }

    /// A DIFFERENT balance must not pass another cell's commitment. This is
    /// the whole reason the hash check exists: without it, any balance could
    /// be asserted for any cell.
    #[test]
    fn a_wrong_amount_cannot_pass_a_cells_commitment() {
        let reg = Registry::default();
        let program = include_bytes!("../fixtures/kcom_token_b.bin").as_slice();
        let st = decode_token_state(&reg, 0, program).unwrap();

        let mut spk = vec![0xaa, 0x20];
        spk.extend_from_slice(&blake2b_256(program));
        spk.push(0x87);

        let lie = 999_999_999u64.to_le_bytes();
        assert!(
            prove_output_state_any(&reg, program, &spk, &st.owner, st.identifier_type, &lie, 0)
                .is_none(),
            "a fabricated amount must never prove against a real commitment"
        );

        let thief = [0x11u8; 32];
        let mut amount = [0u8; 8];
        amount.copy_from_slice(&st.amount_raw[..8]);
        assert!(
            prove_output_state_any(&reg, program, &spk, &thief, st.identifier_type, &amount, 0)
                .is_none(),
            "a fabricated owner must never prove against a real commitment"
        );
    }

    /// The base build keeps its original path untouched.
    #[test]
    fn the_base_build_still_proves_through_the_original_splice() {
        let reg = Registry::default();
        let program = include_bytes!("../fixtures/kcc20_a_a.bin").as_slice();
        if let Some(st) = decode_token_state(&reg, 0, program) {
            let mut amount = [0u8; 8];
            amount.copy_from_slice(&st.amount_raw[..8]);
            let minter = *st.minter_raw.first().unwrap_or(&0);
            let mut spk = vec![0xaa, 0x20];
            spk.extend_from_slice(&blake2b_256(program));
            spk.push(0x87);
            assert!(
                prove_output_state_any(
                    &reg, program, &spk, &st.owner, st.identifier_type, &amount, minter
                )
                .is_some(),
                "the base build must still prove"
            );
        }
    }
}

#[cfg(test)]
mod recovery_base_tests {
    use super::*;

    /// Recovery collects splice BASES before it can prove anything, and it
    /// used to accept only the base build's fixed-offset layout. Every other
    /// family was skipped, `bases` came out empty, and the whole transaction
    /// was passed over — which is why live cells of a launchpad build could
    /// never be proven no matter what the proof path supported.
    #[test]
    fn a_launchpad_cell_is_usable_as_a_splice_base() {
        let reg = Registry::default();
        let program = include_bytes!("../fixtures/kcom_token_b.bin").as_slice();

        assert!(!has_state_block(program), "not the base build's layout");
        assert!(
            is_spliceable_cell(&reg, program),
            "but it must still be spliceable"
        );

        let base = canonical_cell_base(&reg, program).expect("must canonicalise");
        assert_eq!(base.len(), program.len(), "canonicalising preserves length");
        assert_ne!(base, program, "state must actually be zeroed");

        // Canonicalising only zeroes the PER-CELL state. Token-level fields
        // (ticker, token id, mint price) stay, so two cells of one token
        // collapse to one base while cells of DIFFERENT tokens keep distinct
        // bases — which is required: a base carrying the wrong token identity
        // could never splice to a matching commitment anyway.
        let other = include_bytes!("../fixtures/kcom_token_a.bin").as_slice();
        let other_base = canonical_cell_base(&reg, other).expect("must canonicalise");
        assert_ne!(
            other_base, base,
            "different tokens must not share a canonical base"
        );
        // canonicalising is idempotent, so repeated cells collapse
        assert_eq!(
            canonical_cell_base(&reg, &base).expect("base recanonicalises"),
            base,
            "canonicalisation must be stable"
        );

        // and the base still proves the real cell when spliced with its own
        // state, which is what makes it usable at all
        let st = decode_token_state(&reg, 0, program).unwrap();
        let mut amount = [0u8; 8];
        amount.copy_from_slice(&st.amount_raw[..8]);
        let mut spk = vec![0xaa, 0x20];
        spk.extend_from_slice(&blake2b_256(program));
        spk.push(0x87);
        assert!(
            prove_output_state_any(&reg, &base, &spk, &st.owner, st.identifier_type, &amount, 0)
                .is_some(),
            "the canonical base must prove the cell it came from"
        );
    }

    /// A program that is not a cell must not be offered as a base.
    #[test]
    fn a_non_cell_program_is_not_a_base() {
        let reg = Registry::default();
        let auction = include_bytes!("../fixtures/g0_auction_a.bin").as_slice();
        assert!(!is_spliceable_cell(&reg, auction));
        assert!(canonical_cell_base(&reg, auction).is_none());
    }
}

#[cfg(test)]
mod build_aware_base_tests {
    use super::*;

    /// The P2SH commitment a cell with exactly this program carries.
    fn commitment(program: &[u8]) -> Vec<u8> {
        let mut spk = vec![0xaa, 0x20];
        spk.extend_from_slice(&blake2b_256(program));
        spk.push(0x87);
        spk
    }

    fn hex32(s: &str) -> [u8; 32] {
        hex::decode(s).unwrap().try_into().unwrap()
    }

    /// Five skeletons share the "Zealous · token cell" name and the lookup
    /// used to take the first of them by name, so every parent of another
    /// build failed `slot_ranges`, yielded no base, and its live children
    /// were never even attempted. Each build must resolve to its own layout:
    /// the 1,811 B build keeps a 32-byte template-hash push ahead of the
    /// owner, the two shipped builds open with the owner directly.
    #[test]
    fn zealous_skeleton_lookup_is_build_aware() {
        let reg = Registry::default();
        let builds: [(&str, &[u8], Range<usize>); 3] = [
            (
                "zealous_token_1811_a",
                include_bytes!("../fixtures/zealous_token_1811_a.bin"),
                35..67,
            ),
            (
                "zealous_launch_b_a",
                include_bytes!("../fixtures/zealous_launch_b_a.bin"),
                2..34,
            ),
            (
                "zealous_launch_a_a",
                include_bytes!("../fixtures/zealous_launch_a_a.bin"),
                2..34,
            ),
        ];
        for (name, program, owner_range) in builds {
            assert_eq!(
                reg.decode(0, program).template,
                Some("Zealous · token cell"),
                "{name}: one family name for every build"
            );
            let splicer = CellSplicer::new(&reg, program)
                .unwrap_or_else(|| panic!("{name}: the skeleton of its own build must fit"));
            assert_eq!(splicer.owner, owner_range, "{name}: owner slot");
            assert!(!splicer.has_minter_slot(), "{name}: no minter flag in this family");

            let bases = canonical_cell_bases(&reg, program);
            assert_eq!(bases.len(), 1, "{name}: no mode slot, so exactly one base");
            assert_eq!(bases[0].len(), program.len(), "{name}: length preserved");
            assert_eq!(&bases[0][owner_range.clone()], &[0u8; 32], "{name}: owner zeroed");

            // and the base proves the cell it came from, byte for byte: the
            // whole reason a base is worth preparing
            let st = decode_token_state(&reg, 0, program).expect(name);
            let mut amount = [0u8; 8];
            amount.copy_from_slice(&st.amount_raw[..8]);
            let from_base = CellSplicer::new(&reg, &bases[0]).expect(name);
            assert_eq!(from_base.owner, owner_range, "{name}: same layout on the base");
            let proven = from_base
                .prove(&commitment(program), &st.owner, st.identifier_type, &amount, 0)
                .unwrap_or_else(|| panic!("{name}: own state must prove"));
            assert_eq!(proven.owner, st.owner);
            assert_eq!(
                from_base.program_for(&st.owner, st.identifier_type, &amount, 0).as_deref(),
                Some(program),
                "{name}: the proven program is the cell's own"
            );
        }
    }

    /// KaspaCom labels mint_mode as its fourth slot in both generations, and
    /// a mint flips it: the remainder keeps 2 while every cell the mint pays
    /// out carries 0. A base taken from a parent must therefore come in two
    /// variants, and only when the parent's mode is set: a parent at 0 would
    /// yield the same bytes twice. Against the real 56fc521e:0 parent each
    /// variant reaches exactly one of the two live cells its spend created.
    #[test]
    fn kaspacom_mode_slot_yields_two_bases() {
        let reg = Registry::default();
        // mode already 0 in both generations: one base
        for (name, program) in [
            ("kcom_token_a", include_bytes!("../fixtures/kcom_token_a.bin").as_slice()),
            ("kcom_token_2671_a", include_bytes!("../fixtures/kcom_token_2671_a.bin")),
        ] {
            assert_eq!(&program[53..61], &[0u8; 8], "{name}: mint_mode is 0");
            assert_eq!(canonical_cell_bases(&reg, program).len(), 1, "{name}");
        }
        // mode 2 in both generations: two bases differing only at [53..61]
        let parent = include_bytes!("../fixtures/recovery/kcom_2671_parent_56fc521e_0.bin");
        for (name, program) in [
            ("kcom_token_b", include_bytes!("../fixtures/kcom_token_b.bin").as_slice()),
            ("kcom_token_2671_b", include_bytes!("../fixtures/kcom_token_2671_b.bin")),
            ("56fc521e:0", parent.as_slice()),
        ] {
            assert_eq!(&program[53..61], &2u64.to_le_bytes(), "{name}: mint_mode is 2");
            let bases = canonical_cell_bases(&reg, program);
            assert_eq!(bases.len(), 2, "{name}: as-is plus mode-zeroed");
            let differing: Vec<usize> = (0..program.len())
                .filter(|&i| bases[0][i] != bases[1][i])
                .collect();
            assert!(
                !differing.is_empty() && differing.iter().all(|i| (53..61).contains(i)),
                "{name}: the variants differ only in the mode slot, got {differing:?}"
            );
            assert_eq!(&bases[0][53..61], &program[53..61], "{name}: first keeps the mode");
            assert_eq!(&bases[1][53..61], &[0u8; 8], "{name}: second zeroes it");
            for base in &bases {
                assert_eq!(&base[2..34], &[0u8; 32], "{name}: owner zeroed in both");
                assert_eq!(
                    canonical_cell_bases(&reg, base).first(),
                    Some(base),
                    "{name}: each variant is its own as-is base"
                );
            }
        }

        // the analysts' parent hashes to its own commitment
        assert_eq!(
            hex::encode(blake2b_256(parent)),
            "7cec521a9350d30408dae91466aeedc31fa0f34b43d30d6d23c06f650f333820"
        );
        let bases = canonical_cell_bases(&reg, parent);
        let kept = CellSplicer::new(&reg, &bases[0]).expect("mode-kept base prepares");
        let zeroed = CellSplicer::new(&reg, &bases[1]).expect("mode-zeroed base prepares");
        assert!(!kept.has_minter_slot() && !zeroed.has_minter_slot());
        let owner = hex32("51fb56d74326e9f92d4866907994cb6442791f71f2a1a2fee0d408f1cdfa28d1");

        // 7d5f35f0:0, the minted cell: 1 TESTBE at mint_mode 0
        let minted = 100_000_000u64.to_le_bytes();
        let minted_program = zeroed.program_for(&owner, 0, &minted, 0).unwrap();
        assert_eq!(
            hex::encode(blake2b_256(&minted_program)),
            "c0f06ce47fb7331dfff85508e5545b140fff8448c7ff0764c91f5fc549581e56"
        );
        let minted_spk = commitment(&minted_program);
        assert!(zeroed.prove(&minted_spk, &owner, 0, &minted, 0).is_some());
        assert!(
            kept.prove(&minted_spk, &owner, 0, &minted, 0).is_none(),
            "the mode-kept base cannot reach a paid-out cell"
        );

        // 7d5f35f0:1, the remainder: parent amount minus the mint, mode kept
        let rest = 99_948_700_000_000u64.to_le_bytes();
        let rest_program = kept.program_for(&owner, 0, &rest, 0).unwrap();
        assert_eq!(
            hex::encode(blake2b_256(&rest_program)),
            "e41db11e44e0574111cdb44a3a8d063f09ce781809cc78cbfddee9d0e5084e6a"
        );
        let rest_spk = commitment(&rest_program);
        assert!(kept.prove(&rest_spk, &owner, 0, &rest, 0).is_some());
        assert!(
            zeroed.prove(&rest_spk, &owner, 0, &rest, 0).is_none(),
            "the mode-zeroed base cannot reach the remainder"
        );
        // and the proven states carry no minter, as the family has none
        let st = kept.prove(&rest_spk, &owner, 0, &rest, 0).unwrap();
        assert!(st.minter_raw.is_empty());
        assert_eq!(st.amount_i64(), Some(99_948_700_000_000));
    }

    /// The minter axis exists only on the base build. On a family without
    /// the slot every minter value splices the same bytes, which is what a
    /// candidate search must know to avoid hashing each candidate twice.
    #[test]
    fn has_minter_slot_follows_the_build() {
        let reg = Registry::default();
        let kcc20 = include_bytes!("../fixtures/kcc20_a_a.bin").as_slice();
        assert!(CellSplicer::new(&reg, kcc20).unwrap().has_minter_slot());
        for base in canonical_cell_bases(&reg, kcc20) {
            assert!(CellSplicer::new(&reg, &base).unwrap().has_minter_slot());
        }

        let owner = [0x5au8; 32];
        let amount = 7u64.to_le_bytes();
        for (name, program) in [
            ("kcom_token_b", include_bytes!("../fixtures/kcom_token_b.bin").as_slice()),
            ("kcom_token_2671_b", include_bytes!("../fixtures/kcom_token_2671_b.bin")),
            ("zealous_launch_a_a", include_bytes!("../fixtures/zealous_launch_a_a.bin")),
            ("zealous_token_1811_a", include_bytes!("../fixtures/zealous_token_1811_a.bin")),
        ] {
            let splicer = CellSplicer::new(&reg, program).expect(name);
            assert!(!splicer.has_minter_slot(), "{name}");
            assert_eq!(
                splicer.program_for(&owner, 0, &amount, 0),
                splicer.program_for(&owner, 0, &amount, 1),
                "{name}: minter 0 and 1 splice identical bytes"
            );
            for base in canonical_cell_bases(&reg, program) {
                assert!(!CellSplicer::new(&reg, &base).expect(name).has_minter_slot(), "{name}");
            }
        }
    }
}

#[cfg(test)]
mod splice_cost_tests {
    use super::*;

    /// Recovery hashes candidates inside a wall-clock budget, so the cost of
    /// ONE splice attempt is a correctness concern, not a nicety: when it was
    /// dominated by re-deriving every skeleton, the pass burned its three
    /// seconds after ~1,100 attempts and gave up with cells still unproven.
    /// This pins the attempt cost low enough that the hash budget, not the
    /// clock, is what bounds the search.
    #[test]
    fn a_splice_attempt_is_cheap_enough_for_the_recovery_budget() {
        let reg = Registry::default();
        let program = include_bytes!("../fixtures/kcom_token_b.bin").as_slice();
        let st = decode_token_state(&reg, 0, program).unwrap();
        let mut amount = [0u8; 8];
        amount.copy_from_slice(&st.amount_raw[..8]);

        // production prepares the plan once per base, then reuses it
        let splicer = CellSplicer::new(&reg, program).expect("prepares");
        let mut spk = vec![0xaa, 0x20];
        spk.extend_from_slice(&blake2b_256(program));
        spk.push(0x87);

        let n = 200;
        let start = std::time::Instant::now();
        for i in 0..n {
            let mut owner = st.owner;
            owner[0] = i as u8; // a fresh wrong candidate every time
            std::hint::black_box(splicer.prove(&spk, &owner, st.identifier_type, &amount, 0));
        }
        let per = start.elapsed() / n;
        assert!(
            per < std::time::Duration::from_micros(200),
            "one splice attempt took {per:?}; recovery's 3s budget would allow only \
             ~{} attempts, and the pass needs thousands",
            3_000_000 / per.as_micros().max(1)
        );
    }
}
