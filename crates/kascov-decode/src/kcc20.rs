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
}

/// Decode `program` as a KCC20 token state via the registry skeletons.
/// Returns the four labeled fields only when the template is "KCC20 token"
/// and every field is present with its observed width (owner 32 bytes,
/// identifier_type 1 byte) — a partial or misshapen decode yields `None`.
pub fn decode_token_state(
    registry: &Registry,
    spk_version: u16,
    program: &[u8],
) -> Option<TokenState> {
    let d = registry.decode(spk_version, program);
    if d.template == Some(TOKEN_TEMPLATE) {
        let field = |name: &str| {
            d.fields
                .iter()
                .find(|f| f.name == name)
                .map(|f| f.value.clone())
        };
        let owner: [u8; 32] = field("owner_identifier")?.try_into().ok()?;
        let id_type = field("identifier_type")?;
        let [identifier_type] = id_type.as_slice() else {
            return None;
        };
        return Some(TokenState {
            owner,
            identifier_type: *identifier_type,
            amount_raw: field("amount")?,
            minter_raw: field("is_minter")?,
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

/// KCC-1 draft §8.3 TemplateHash of a program carrying the verified KCC20
/// state block: prefix is the leading alt-stack guard byte, the state range
/// is bytes [1, 47), suffix is everything from the closing guard on. `None`
/// when the block is absent — the canonical hash is only computed where the
/// state range is proven, never guessed. Derivation pinned to spec commit
/// 55b28d8; recompute is gated by the store's `kcc1_abi_version` meta.
pub fn kcc1_template_hash(program: &[u8]) -> Option<[u8; 32]> {
    let b = locate_state_block(program)?;
    Some(crate::kcc1::template_hash(
        &program[..b.start],
        &program[b.end()..],
    ))
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
