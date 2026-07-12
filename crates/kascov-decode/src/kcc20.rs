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

/// Decode `program` as a KCC20 token state via the registry skeletons.
/// Returns the four labeled fields only when the template is "KCC20 token"
/// and every field is present with its observed width (owner 32 bytes,
/// identifier_type 1 byte) — a partial or misshapen decode yields `None`.
pub fn decode_token_state(registry: &Registry, spk_version: u16, program: &[u8]) -> Option<TokenState> {
    let d = registry.decode(spk_version, program);
    if d.template != Some(TOKEN_TEMPLATE) {
        return None;
    }
    let field = |name: &str| d.fields.iter().find(|f| f.name == name).map(|f| f.value.clone());
    let owner: [u8; 32] = field("owner_identifier")?.try_into().ok()?;
    let id_type = field("identifier_type")?;
    let [identifier_type] = id_type.as_slice() else { return None };
    Some(TokenState {
        owner,
        identifier_type: *identifier_type,
        amount_raw: field("amount")?,
        minter_raw: field("is_minter")?,
    })
}

/// Does `program` open with the fixed KCC20 state block (see module docs)?
pub fn has_state_block(program: &[u8]) -> bool {
    program.len() >= 48
        && program[0] == 0x6b
        && program[1] == 0x20
        && program[34] == 0x01
        && program[36] == 0x08
        && program[45] == 0x01
        && program[47] == 0x6c
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
    if !has_state_block(program) {
        return None;
    }
    let mut p = program.to_vec();
    p[2..34].copy_from_slice(owner);
    p[35] = identifier_type;
    p[37..45].copy_from_slice(amount);
    p[46] = is_minter;
    Some(p)
}

/// blake2b-256 — the hash Kaspa P2SH commitments use (same parameters as
/// [`crate::p2sh_reveal`]'s verification).
pub fn blake2b_256(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(blake2b_simd::Params::new().hash_length(32).hash(bytes).as_bytes());
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
