//! ARGENT build recognition: deciding, from bytes alone, whether a revealed
//! program came out of argent's code generator, which generation of the
//! SilverScript compiler lowered it, and cutting it into the (head, state,
//! code) split whose §8.3 TemplateHash names its BUILD.
//!
//! Why this is not just "look for the dispatch tail". A silverscript program
//! that dispatches compiles to
//!
//! ```text
//!   6b <state pushes> 6c                         alt-stack guarded state
//!   76 <rung> 63 75  <branch body>  67   ...     one rung per entrypoint
//!   <terminator> 68 68 … 68                      nothing matched: fail
//! ```
//!
//! and that envelope is SILVERSCRIPT's, not argent's — KRON's curve builds
//! carry it byte for byte. Recognising the envelope alone would have called
//! every KRON market an argent app. What argent's code generator adds, and
//! what nothing else observed on chain emits, is the LEADER PREAMBLE at the
//! top of each branch: `b9 cb <N> 9c 69` (assert this input authorises
//! exactly N outputs) followed by N distinct `b9 <j> cc` sites pinning each
//! one. Both halves are required, which is what makes the test survive
//! contact with real chain data — measured over every landed spend on
//! testnet-10 and mainnet, 0 misses across the 541 reveals of the 29
//! covenants making up the six known Zealous families, and 0 false positives
//! across 12,650 KRON spends.
//!
//! TWO GENERATIONS of the rung, and the rung form is what dates a build:
//!
//! - The silverscript FORK (michaelsutton/silverscript, the compiler the
//!   Zealous families were built with and the one kascov's own `/compile`
//!   runs) selects a branch by a small-int selector: `76 <OP_0..OP_16> 9c 63
//!   75`, selectors ascending from zero. Its template hash is BLAKE2b, the
//!   hash an `OP_BLAKE2B` program computes in-script, which is what every
//!   deployed argent covenant on testnet-10 checks.
//! - SilverScript 1.0 (kaspanet/silverscript 3ed9733) selects by a 4-byte
//!   dispatch tag, `blake3("name(types)")[..4]`: `76 04 <tag> 87 63 75`
//!   (silverscript-lang/src/compiler/compile.rs:352-391, the tag at
//!   compile/helpers.rs:155-158). Its template hash is BLAKE3 under the same
//!   LE64 framing (`crate::kcc1::template_hash`), checked in-script with
//!   `OpBlake3`. 1.0 always guards a stateful contract's state with `6b … 6c`,
//!   so a guardless leaf is never 1.0.
//!
//! The two arms hash the same bytes to different numbers, so a program's
//! generation is read BEFORE its hash is computed, never guessed from the
//! hash, and [`Argent::generation`] says which arm named it.
//!
//! Rules that look obvious and are WRONG on real bytes, learned the expensive
//! way from the census and from compiling the same contract through three
//! compilers:
//!
//! 1. The terminator is a compiler REVISION, not an identity. Exactly two
//!    exist on either chain, `6a` and `75 00 69`. `6a` is what the fork the
//!    box runs today (d57e5dff) emits and what 1.0 emits; `75 00 69` is the
//!    older revision (kaspanet/silverscript d25bd34, the generation the six
//!    embedded skeleton dumps came from). Keying on either alone admits every
//!    KRON build. Worth stating what the census does NOT show: every Zealous
//!    reveal is op-return (499) or a leaf (42), so not one of them exercises
//!    the older tail. It is carried for programs that revision compiled, not
//!    for them.
//! 2. The authorised-output indices are a PERMUTATION of `0..N-1`, not an
//!    ascending run. Zealous's own token cell emits `(1, 0)` in one branch
//!    and declares `N = 0` in another; demanding ascending order and `N >= 1`
//!    rejects the single largest argent family on the chain.
//! 3. An entry may authorise a bounded RANGE of outputs rather than an exact
//!    count (argent 8b010f3, "bounded ranges to entry consumes and emits").
//!    Its preamble is `b9 cb [<k> 94] 76 <min> a2 69 76 <max> a1 69`: the
//!    count less the k fixed handles must lie in `min..=max`, and the sites
//!    behind the range index by computed operands (`count - 1`, `1 + i`)
//!    instead of literals. Such an entry is surfaced as a range, never as a
//!    single number, and only its literal sites are held to being pinned
//!    once.
//!
//! What a match licenses saying is narrow and worth stating: this program was
//! compiled by argent's leader codegen through this generation of
//! silverscript, its template hash under that generation's arm is X, it
//! declares these entrypoints, and each authorises this many outputs. It does
//! NOT say what the program is FOR. Role is unknown unless separately proven;
//! kascov stores exactly these facts and nothing
//! that resembles a price.

/// The FORK-ERA template hash: what an argent app built with the silverscript
/// fork commits to, and the primitive that makes covenant composability work
/// without a vendor SDK.
///
/// An argent contract stores a 32-byte commitment to a *template* — the
/// immutable code around a mutable state window — rather than to any specific
/// program. A spender supplies the template halves in the witness, the
/// covenant hashes them and checks the commitment, and only then reads the
/// state. So a market can pair with any token on that template without ever
/// knowing the token statically. Michael Sutton described this shape publicly
/// on 2026-08-12; Zealous Swap already runs it on testnet-10.
///
/// Same framing as KCC-1 §8.3 (`LE64(len(prefix)) || prefix || LE64(len(suffix))
/// || suffix`) but under BLAKE2b, because that is the hash a fork-era program
/// computes in-script (`OP_BLAKE2B`) and what the deployed Zealous bytes
/// check. SilverScript 1.0 programs commit to the BLAKE3 value instead
/// (`crate::kcc1::template_hash`, checked with `OpBlake3`); the two are
/// siblings, chosen by [`Generation::template_hash`] from the program's rung
/// form. This function must keep BLAKE2b: a re-pin here would stop every
/// live fork-era commitment from verifying. The two store columns
/// (`argent_template_hash`, `kcc1_template_hash`) are siblings for the same
/// reason.
pub fn template_hash(prefix: &[u8], suffix: &[u8]) -> [u8; 32] {
    let mut state = blake2b_simd::Params::new().hash_length(32).to_state();
    state.update(&(prefix.len() as u64).to_le_bytes());
    state.update(prefix);
    state.update(&(suffix.len() as u64).to_le_bytes());
    state.update(suffix);
    let mut out = [0u8; 32];
    out.copy_from_slice(state.finalize().as_bytes());
    out
}

const OP_0: u8 = 0x00;
const OP_DATA_4: u8 = 0x04;
const OP_PUSHDATA1: u8 = 0x4c;
const OP_PUSHDATA2: u8 = 0x4d;
const OP_PUSHDATA4: u8 = 0x4e;
const OP_1: u8 = 0x51;
const OP_16: u8 = 0x60;
const OP_IF: u8 = 0x63;
const OP_NOTIF: u8 = 0x64;
const OP_ELSE: u8 = 0x67;
const OP_ENDIF: u8 = 0x68;
const OP_VERIFY: u8 = 0x69;
const OP_RETURN: u8 = 0x6a;
const OP_TOALTSTACK: u8 = 0x6b;
const OP_FROMALTSTACK: u8 = 0x6c;
const OP_DROP: u8 = 0x75;
const OP_DUP: u8 = 0x76;
const OP_EQUAL: u8 = 0x87;
const OP_SUB: u8 = 0x94;
const OP_NUMEQUAL: u8 = 0x9c;
const OP_LESSTHANOREQUAL: u8 = 0xa1;
const OP_GREATERTHANOREQUAL: u8 = 0xa2;
/// `this.activeInputIndex`.
const OP_ACTIVE_INPUT_INDEX: u8 = 0xb9;
/// `OpAuthOutputCount` — how many outputs this input authorises.
const OP_AUTH_OUTPUT_COUNT: u8 = 0xcb;
/// `OpAuthOutputIdx` — the index of one authorised output.
const OP_AUTH_OUTPUT_IDX: u8 = 0xcc;
/// `OpCovOutputCount` — how many outputs continue a covenant. From argentc
/// e76ee07 on, a coordinated leader entry that consumes inputs compares it
/// with its own `OpAuthOutputCount` (argent's leader/delegate security rule 5).
const OP_COV_OUTPUT_COUNT: u8 = 0xd2;

/// Which compiler generation lowered the program, read off its dispatch rung
/// form, and with it which hash names its template. See the module note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Generation {
    /// Selector rungs, or a guardless leaf: the silverscript fork. The
    /// template hash is BLAKE2b ([`template_hash`]), what an `OP_BLAKE2B`
    /// program checks in-script and what the Zealous families on testnet-10
    /// commit to.
    Fork,
    /// Tag rungs: SilverScript 1.0. The template hash is BLAKE3
    /// (`crate::kcc1::template_hash`), what the program checks with
    /// `OpBlake3`. Always guarded; a leaf is never 1.0.
    V1,
}

impl Generation {
    /// Stable tag for storage and display.
    pub fn as_str(self) -> &'static str {
        match self {
            Generation::Fork => "fork",
            Generation::V1 => "1.0",
        }
    }

    /// The hash function this generation's programs check in-script.
    pub fn hash_name(self) -> &'static str {
        match self {
            Generation::Fork => "BLAKE2b",
            Generation::V1 => "BLAKE3",
        }
    }

    /// `TemplateHash(prefix, suffix)` under this generation's arm: identical
    /// LE64 framing, BLAKE2b for the fork, BLAKE3 for 1.0.
    pub fn template_hash(self, prefix: &[u8], suffix: &[u8]) -> [u8; 32] {
        match self {
            Generation::Fork => template_hash(prefix, suffix),
            Generation::V1 => crate::kcc1::template_hash(prefix, suffix),
        }
    }
}

/// How the dispatch chain fails when no rung matched. Both mean "fail"; they
/// differ only in how, so this dates a compiler revision and never identifies
/// a build. See the module note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terminator {
    /// `67 6a 68…68`: the fork the box runs today (d57e5dff) and 1.0.
    OpReturn,
    /// `67 75 00 69 68…68`: the older revision (kaspanet/silverscript
    /// d25bd34, the generation the embedded skeleton dumps came from).
    DropFail,
}

impl Terminator {
    /// The bytes between the final `OP_ELSE` and the closing `ENDIF` run.
    fn bytes(self) -> &'static [u8] {
        match self {
            Terminator::OpReturn => &[OP_RETURN],
            Terminator::DropFail => &[OP_DROP, OP_0, OP_VERIFY],
        }
    }

    /// Stable tag for storage and display.
    pub fn as_str(self) -> &'static str {
        match self {
            Terminator::OpReturn => "op-return",
            Terminator::DropFail => "drop-fail",
        }
    }
}

/// The complete set observed across every landed spend on both chains. Order
/// matters only in that `6a` is checked first; the two cannot both match, as
/// `75 00 69` does not end in `6a`.
const TERMINATORS: [Terminator; 2] = [Terminator::OpReturn, Terminator::DropFail];

/// How much the bytes prove about where state ends and code begins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Split {
    /// The program carries a dispatch chain, so the `6b … 6c` guard fixes the
    /// state window: the cut is read off the program, not chosen.
    Proven,
    /// A single-entrypoint (leaf) program has no guard and no branch table.
    /// The cut is taken at the first non-push opcode, which is where argent
    /// puts it — but nothing in THESE bytes says so. Only a peer program that
    /// stores `TemplateHash(∅, code)` as a constant confirms it, which held
    /// for 14 of 38 leaf families on testnet-10 and 0 of 24 on mainnet. Label
    /// it wherever it surfaces; never let it pass for proof. Fork era only:
    /// 1.0 guards every stateful contract, so this tier never dates a build
    /// as 1.0.
    Candidate,
}

impl Split {
    pub fn as_str(self) -> &'static str {
        match self {
            Split::Proven => "dispatch",
            Split::Candidate => "leaf-candidate",
        }
    }
}

/// What selects one rung of the dispatch chain: the push a spender puts
/// right before the program in the signature script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dispatch {
    /// A leaf has no chain: the program is its only entry and a spend pushes
    /// no selector at all.
    Leaf,
    /// Fork era: the small-int selector, ascending from zero in rung order.
    Selector(u8),
    /// 1.0: the 4-byte dispatch tag, `blake3("name(types)")[..4]`.
    Tag([u8; 4]),
}

/// What an entry's leader preamble asserts about the outputs its input
/// authorises. A fact about the PROGRAM's assertion, not about any
/// transaction that ran it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Authorised {
    /// `b9 cb <N> 9c 69`: exactly N, each pinned once by a literal site.
    Exact(u8),
    /// `b9 cb [<fixed> 94] 76 <min> a2 69 76 <max> a1 69`: `fixed` handles
    /// plus a bounded run of `min..=max` more, indexed by computed operands.
    Range { fixed: u8, min: u8, max: u8 },
}

impl Authorised {
    /// The exact count, or `None` for a range.
    pub fn exact(self) -> Option<u8> {
        match self {
            Authorised::Exact(n) => Some(n),
            Authorised::Range { .. } => None,
        }
    }
}

/// One dispatch rung: what selects it, and what its leader preamble asserts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entrypoint {
    pub dispatch: Dispatch,
    pub authorised: Authorised,
}

impl Entrypoint {
    /// The fork-era selector, or `None` for a tag rung or a leaf.
    pub fn selector(&self) -> Option<u8> {
        match self.dispatch {
            Dispatch::Selector(s) => Some(s),
            _ => None,
        }
    }

    /// The 1.0 dispatch tag, or `None` for a selector rung or a leaf.
    pub fn tag(&self) -> Option<[u8; 4]> {
        match self.dispatch {
            Dispatch::Tag(t) => Some(t),
            _ => None,
        }
    }
}

/// An argent-compiled program, split and hashed.
///
/// The three lengths partition the program exactly: `head_len + state_len +
/// code_len == program.len()`. Only `head` and `code` feed the template hash
/// — the state window is the part instances are allowed to differ in, which
/// is the whole point of hashing around it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Argent {
    pub split: Split,
    /// Which compiler generation lowered the program, and so which arm
    /// computed `template_hash`.
    pub generation: Generation,
    /// `None` for a leaf: with no dispatch chain there is no terminator to
    /// date the revision by.
    pub terminator: Option<Terminator>,
    pub head_len: usize,
    pub state_len: usize,
    pub code_len: usize,
    pub entrypoints: Vec<Entrypoint>,
    /// §8.3 `TemplateHash(head, code)` under `generation`'s arm: the
    /// identity of the BUILD, shared by every instance whatever its state
    /// says.
    pub template_hash: [u8; 32],
    /// End offset of each state push, for [`Argent::candidate_splits`].
    state_push_ends: Vec<usize>,
}

impl Argent {
    pub fn entrypoint_count(&self) -> usize {
        self.entrypoints.len()
    }

    /// Re-slice the program this was recognised from: `(head, state, code)`.
    /// Panics if handed a different program — the offsets are only meaningful
    /// against the bytes they were read from.
    pub fn slices<'a>(&self, program: &'a [u8]) -> (&'a [u8], &'a [u8], &'a [u8]) {
        assert_eq!(
            program.len(),
            self.head_len + self.state_len + self.code_len,
            "slices() must be handed the program this split was read from"
        );
        let (head, rest) = program.split_at(self.head_len);
        let (state, code) = rest.split_at(self.state_len);
        (head, state, code)
    }

    /// Every head boundary a peer's commitment could legally be pinning, with
    /// the hash each one yields under this program's own arm.
    ///
    /// A composing program may commit to a NARROWER state window than the one
    /// the guard marks — it only has to fix the fields it cares about, and
    /// everything before them counts as head. So when `template_hash` does not
    /// match a constant found on chain, the answer is often one of these: the
    /// head grows one state push at a time, the code half never moves. This is
    /// how the 12,046 B Zealous market's otherwise-unexplained `9ecf15f3…`
    /// commitment resolves, at a 67-byte head. A 1.0 actor-type handle is the
    /// same walk under BLAKE3: the sil prefix extended by the context-field
    /// pushes.
    pub fn candidate_splits(&self, program: &[u8]) -> Vec<(usize, [u8; 32])> {
        let code_start = self.head_len + self.state_len;
        let code = &program[code_start..];
        std::iter::once(self.head_len)
            .chain(self.state_push_ends.iter().copied())
            .map(|boundary| {
                (
                    boundary,
                    self.generation.template_hash(&program[..boundary], code),
                )
            })
            .collect()
    }
}

/// One decoded opcode: where the next one starts, the opcode byte, and its
/// pushed payload length when it is a data push.
///
/// `OP_0` counts as a data push (it pushes the empty payload); `OP_1NEGATE`
/// and `OP_1..OP_16` deliberately do NOT, because in a state block a numeric
/// opcode is a logic byte, not a field, and admitting them would let the
/// state/code cut slide.
fn step(program: &[u8], at: usize) -> Option<(usize, u8, Option<usize>)> {
    let op = *program.get(at)?;
    let after = at + 1;
    let (len, header) = match op {
        OP_0 => return Some((after, op, Some(0))),
        0x01..=0x4b => (op as usize, 1),
        OP_PUSHDATA1 => (*program.get(after)? as usize, 2),
        OP_PUSHDATA2 => (
            u16::from_le_bytes(program.get(after..after + 2)?.try_into().ok()?) as usize,
            3,
        ),
        OP_PUSHDATA4 => (
            u32::from_le_bytes(program.get(after..after + 4)?.try_into().ok()?) as usize,
            5,
        ),
        _ => return Some((after, op, None)),
    };
    let end = at.checked_add(header)?.checked_add(len)?;
    (end <= program.len()).then_some((end, op, Some(len)))
}

/// The payloads of a state window that is nothing but data pushes, in order.
/// `None` when a logic byte sits inside it, a push runs past its end, or the
/// window is empty: a state window an artifact describes field by field is
/// exactly a run of pushes, and anything else is not the window the fields
/// were declared over.
pub fn pushes(window: &[u8]) -> Option<Vec<&[u8]>> {
    let mut at = 0usize;
    let mut out = Vec::new();
    while at < window.len() {
        let (next, op, data) = step(window, at)?;
        let len = data?;
        let payload_start = next - len;
        // `OP_0` pushes the empty payload; every other push carries its
        // payload as the tail of the instruction.
        out.push(if op == OP_0 { &window[at..at] } else { &window[payload_start..next] });
        at = next;
    }
    (!out.is_empty()).then_some(out)
}

/// A pushed script number as the engine reads it: little-endian magnitude,
/// sign in the top bit of the last byte, at most eight bytes, the empty push
/// being zero. Minimal encoding is deliberately NOT demanded. An argent state
/// int on chain is written by `OpNum2Bin 8` (the `01 08 … 58 cd` sequence in
/// every generated program), so a live `5` is `05 00 00 00 00 00 00 00`, and a
/// reader that refused padding would refuse every real value.
pub fn script_num(payload: &[u8]) -> Option<i64> {
    if payload.is_empty() {
        return Some(0);
    }
    if payload.len() > 8 {
        return None;
    }
    let mut bytes = [0u8; 8];
    bytes[..payload.len()].copy_from_slice(payload);
    let negative = payload[payload.len() - 1] & 0x80 != 0;
    bytes[payload.len() - 1] &= 0x7f;
    let magnitude = i64::from_le_bytes(bytes);
    Some(if negative { -magnitude } else { magnitude })
}

/// `OP_0` or `OP_1..OP_16` read as the number they push. Anything else is
/// `None`: argent's codegen only ever emits selectors, output counts and
/// range bounds as these one-byte literals, so a data push here is a
/// different program.
fn small_int(program: &[u8], at: usize) -> Option<u8> {
    match *program.get(at)? {
        OP_0 => Some(0),
        op @ OP_1..=OP_16 => Some(op - OP_1 + 1),
        _ => None,
    }
}

/// The silverscript dispatch envelope: a trailing `ENDIF` run preceded by a
/// known terminator. Returns the rung count and the terminator.
///
/// Public because a negative result is worth being able to state precisely:
/// KRON's curve builds pass THIS and are argent only if they also pass
/// [`recognize`]. A caller that treats this as recognition has widened the
/// match to all of silverscript.
pub fn dispatch_envelope(program: &[u8]) -> Option<(usize, Terminator)> {
    let mut at = program.len();
    while at > 0 && program[at - 1] == OP_ENDIF {
        at -= 1;
    }
    let rungs = program.len() - at;
    if rungs == 0 {
        return None;
    }
    let terminator = TERMINATORS
        .into_iter()
        .find(|t| program[..at].ends_with(t.bytes()))?;
    Some((rungs, terminator))
}

/// The bound checks of a ranged preamble, starting at `at`: `76 <min> a2 69
/// 76 <max> a1 69`, with `min <= max` as a compiler would only ever emit.
fn range_bounds(program: &[u8], at: usize) -> Option<(u8, u8)> {
    if *program.get(at)? != OP_DUP {
        return None;
    }
    let min = small_int(program, at + 1)?;
    if program.get(at + 2..at + 4)? != [OP_GREATERTHANOREQUAL, OP_VERIFY] {
        return None;
    }
    if *program.get(at + 4)? != OP_DUP {
        return None;
    }
    let max = small_int(program, at + 5)?;
    if program.get(at + 6..at + 8)? != [OP_LESSTHANOREQUAL, OP_VERIFY] {
        return None;
    }
    (min <= max).then_some((min, max))
}

/// What a `b9 cb` at `at` (the `cb`) asserts when it is a COUNT SITE:
/// `cb <N> 9c 69` asserts exactly N; `cb <k> 94` followed by the bound checks
/// asserts a range behind k fixed handles; `cb` straight into the bound
/// checks asserts a range with no fixed handle (argent's emitter writes
/// `count - k` only when k is nonzero). `None` when the `cb` is neither: an
/// operand of a computed index, which only a range branch has.
fn count_site(program: &[u8], at: usize) -> Option<Authorised> {
    match small_int(program, at + 1) {
        Some(n) if program.get(at + 2..at + 4)? == [OP_NUMEQUAL, OP_VERIFY] => Some(Authorised::Exact(n)),
        Some(fixed) if *program.get(at + 2)? == OP_SUB => {
            range_bounds(program, at + 3).map(|(min, max)| Authorised::Range { fixed, min, max })
        }
        Some(_) => None,
        None => range_bounds(program, at + 1).map(|(min, max)| Authorised::Range { fixed: 0, min, max }),
    }
}

/// Walk one branch body to the `OP_ELSE` that closes it, checking the leader
/// preamble on the way. Returns `(else_offset, authorised)`.
///
/// Depth tracking is what keeps this honest: the preamble has to sit at the
/// branch's own top level, not inside some nested conditional where it would
/// only sometimes run. Every one of the 15,791 accepted programs on both
/// chains (1,599 testnet-10 + 14,192 mainnet) satisfies that for free, and
/// the programs it does exclude are the hand-written class this recognizer
/// is meant to leave alone: partial coverage (`4/6` branches carrying a
/// preamble) and `idx = [0, 0]`, which is two pins on one output rather than
/// a permutation.
///
/// A RANGE branch relaxes exactly three things, and only after its count site
/// has been seen: `b9 cb` may recur as the operand of a computed index
/// (`count - 1` for a fixed handle behind the range), `OpAuthOutputIdx` may
/// take a computed operand at any depth (the range members are pinned inside
/// the unrolled loop's conditionals), and a literal site may READ a range
/// member the count guarantees, `fixed <= j < fixed + min`, at any depth.
/// The last is what the KCC20 reference actor (argent-lang/kcc20-reference
/// c8a0871) compiles its borrowed path to: `KCC20[1..=3]` outputs, and inside
/// the borrow arm `next_outputs[0].value >= self.value` as `b9 00 cc` three
/// conditionals deep. That site names an output the range already pinned, so
/// it is not counted as a fixed handle. The fixed handles' own literal sites
/// are still held to being pinned once each, at branch top level.
///
/// Since argentc e76ee07 a coordinated leader entry that consumes inputs also
/// restates its count against its covenant group's (argent's security rule
/// 5): `require(OpCovOutputCount(cov) == OpAuthOutputCount(active))`, which
/// compiles to `… d2 b9 cb 9c 69`. That `b9 cb` is an operand of the
/// comparison, not a count site. It is read once per branch, at the branch's
/// top level and after the count site it restates, with `OpCovOutputCount`
/// and the active index as the two instructions right before it and
/// `OpNumEqual OpVerify` right after. A second one, or one ahead of the count
/// site, is not what the compiler writes and leaves the branch unread.
fn scan_branch(program: &[u8], body: usize) -> Option<(usize, Authorised)> {
    let mut depth = 1usize;
    let mut at = body;
    let mut authorised: Option<Authorised> = None;
    let mut literal: Vec<u8> = Vec::new();
    // The two instructions before the one being read, as (offset, opcode,
    // is a data push), so an operand is matched on instructions rather than
    // on bytes that might sit inside a push.
    let mut prev: Option<(usize, u8, bool)> = None;
    let mut prev2: Option<(usize, u8, bool)> = None;
    let mut continuation_checked = false;
    let else_at = loop {
        let (next, op, data) = step(program, at)?;
        if data.is_none() {
            let ranged = matches!(authorised, Some(Authorised::Range { .. }));
            match op {
                OP_IF | OP_NOTIF => depth += 1,
                OP_ENDIF => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        // The branch closed without an ELSE: the chain this
                        // rung belongs to cannot continue, so it is not one.
                        return None;
                    }
                }
                OP_ELSE if depth == 1 => break at,
                OP_AUTH_OUTPUT_COUNT if at >= 1 && program[at - 1] == OP_ACTIVE_INPUT_INDEX => {
                    match count_site(program, at) {
                        // Exactly one count site per branch, at branch top
                        // level, asserted immediately.
                        Some(site) => {
                            if depth != 1 || authorised.is_some() {
                                return None;
                            }
                            authorised = Some(site);
                        }
                        // Rule 5: the leader restating its count against the
                        // covenant group's, once, after the count site.
                        None if !continuation_checked
                            && depth == 1
                            && authorised.is_some()
                            && at >= 2
                            && prev == Some((at - 1, OP_ACTIVE_INPUT_INDEX, false))
                            && prev2 == Some((at - 2, OP_COV_OUTPUT_COUNT, false))
                            && program.get(at + 1..at + 3) == Some(&[OP_NUMEQUAL, OP_VERIFY][..]) =>
                        {
                            continuation_checked = true;
                        }
                        // `b9 cb` as an operand. Only a range branch computes
                        // indices, and its count site precedes every such use.
                        None if ranged => {}
                        None => return None,
                    }
                }
                OP_AUTH_OUTPUT_IDX => {
                    let literal_site = at >= 2
                        && program[at - 2] == OP_ACTIVE_INPUT_INDEX
                        && small_int(program, at - 1).is_some();
                    if literal_site {
                        let j = small_int(program, at - 1)?;
                        match authorised {
                            // a read of a member the range guarantees: not a
                            // fixed handle, and allowed at any depth
                            Some(Authorised::Range { fixed, min, .. })
                                if j >= fixed && j < fixed.saturating_add(min) => {}
                            // `b9 <j> cc`, pinning a literal index: branch top
                            // level in either form.
                            _ => {
                                if depth != 1 {
                                    return None;
                                }
                                literal.push(j);
                            }
                        }
                    } else if !ranged {
                        // Anything else reaching OpAuthOutputIdx is computing
                        // an index rather than pinning a literal one, which
                        // outside a range branch is a different (and
                        // unproven) contract.
                        return None;
                    }
                }
                _ => {}
            }
        }
        prev2 = prev;
        prev = Some((at, op, data.is_some()));
        at = next;
    };
    let authorised = authorised?;
    let mut sorted = literal.clone();
    sorted.sort_unstable();
    let pinned_once = match authorised {
        // A PERMUTATION of 0..N-1, in any order (see the module note). Each
        // authorised output pinned exactly once is the invariant; ascending
        // is just the shape most branches happen to have.
        Authorised::Exact(n) => sorted == (0..n).collect::<Vec<u8>>(),
        // The literal sites are the fixed handles in front of the range:
        // distinct, and none of them reaching past the fixed count.
        Authorised::Range { fixed, .. } => {
            sorted.windows(2).all(|w| w[0] != w[1]) && sorted.iter().all(|j| *j < fixed)
        }
    };
    pinned_once.then_some((else_at, authorised))
}

/// One rung after its `OP_DUP`, at `at`: which push selects it, where its
/// body starts, and which generation wrote it. A tag rung is `04 <tag> 87 63
/// 75`; a selector rung is `<OP_n> 9c 63 75` with n the rung's position.
fn rung(program: &[u8], at: usize, index: usize) -> Option<(Dispatch, usize, Generation)> {
    if *program.get(at)? == OP_DATA_4 {
        let tag: [u8; 4] = program.get(at + 1..at + 5)?.try_into().ok()?;
        if program.get(at + 5..at + 8)? != [OP_EQUAL, OP_IF, OP_DROP] {
            return None;
        }
        return Some((Dispatch::Tag(tag), at + 8, Generation::V1));
    }
    // Selectors ascend 0, 1, 2 and so on; a gap or a repeat is a chain the
    // compiler did not emit.
    let selector = u8::try_from(index).ok()?;
    if small_int(program, at)? != selector {
        return None;
    }
    if program.get(at + 1..at + 4)? != [OP_NUMEQUAL, OP_IF, OP_DROP] {
        return None;
    }
    Some((Dispatch::Selector(selector), at + 4, Generation::Fork))
}

/// Tier 1 — an argent program with a dispatch chain, whose state window the
/// `6b … 6c` guard proves. Either generation: the rung form says which.
pub fn recognize_dispatch(program: &[u8]) -> Option<Argent> {
    // The tail gate first: it reads a handful of trailing bytes, so the
    // cheapest test runs before any scan. It refuses 39% of the reveals that
    // reach it (157,589 of 259,098 carry the envelope) — worth naming
    // precisely, because the tempting number is the ~95% of SPENT ROWS it
    // appears to reject, and those rows never reach this function at all.
    let (rungs, terminator) = dispatch_envelope(program)?;

    if *program.first()? != OP_TOALTSTACK {
        return None;
    }
    let mut at = 1usize;
    let mut state_push_ends = Vec::new();
    loop {
        let (next, op, data) = step(program, at)?;
        if op == OP_FROMALTSTACK {
            break;
        }
        // Only data pushes live in a state block; a logic byte here means the
        // guard is doing something other than fencing state.
        data?;
        state_push_ends.push(next);
        at = next;
    }
    let state_end = at;

    at = state_end + 1;
    let mut entrypoints: Vec<Entrypoint> = Vec::with_capacity(rungs);
    let mut generation: Option<Generation> = None;
    for index in 0..rungs {
        if *program.get(at)? != OP_DUP {
            return None;
        }
        let (dispatch, body, form) = rung(program, at + 1, index)?;
        // One chain, one form: neither compiler mixes selector and tag rungs,
        // and a 1.0 chain never repeats a tag (the compiler refuses the
        // collision at build time).
        if generation.is_some_and(|g| g != form) {
            return None;
        }
        generation = Some(form);
        if entrypoints.iter().any(|e| e.dispatch == dispatch) {
            return None;
        }
        let (else_at, authorised) = scan_branch(program, body)?;
        entrypoints.push(Entrypoint {
            dispatch,
            authorised,
        });
        at = else_at + 1;
    }
    // The chain must land exactly on the tail. Without this the rungs could
    // stop early and leave arbitrary bytes running before the terminator.
    if at != program.len() - rungs - terminator.bytes().len() {
        return None;
    }
    let generation = generation?;
    // 1.0 closes its chain with OpReturn (compile.rs:379 at 3ed9733); a tag
    // chain on the older tail is nothing either compiler wrote.
    if generation == Generation::V1 && terminator != Terminator::OpReturn {
        return None;
    }

    let state_len = state_end - 1;
    Some(Argent {
        split: Split::Proven,
        generation,
        terminator: Some(terminator),
        head_len: 1,
        state_len,
        code_len: program.len() - state_end,
        entrypoints,
        template_hash: generation.template_hash(&program[..1], &program[state_end..]),
        state_push_ends,
    })
}

/// Tier 2 — a single-entrypoint (leaf) argent program: a run of data pushes,
/// then a body carrying the leader preamble. No guard, no branch table, so
/// the cut is a [`Split::Candidate`] and must be surfaced as one. Fork era
/// only, and hashed under BLAKE2b: 1.0 guards every stateful contract, so a
/// guardless program was not lowered by it.
pub fn recognize_leaf(program: &[u8]) -> Option<Argent> {
    // A guarded program is tier 1's business; falling through to here would
    // hand a dispatch program a candidate cut it does not need.
    if *program.first()? == OP_TOALTSTACK {
        return None;
    }
    let mut at = 0usize;
    let mut state_push_ends = Vec::new();
    loop {
        let (next, _, data) = step(program, at)?;
        if data.is_none() {
            break;
        }
        state_push_ends.push(next);
        at = next;
        if at >= program.len() {
            // All pushes and no body: data, not a program.
            return None;
        }
    }
    if state_push_ends.is_empty() {
        return None;
    }
    let cut = at;

    // No depth gate here: with no dispatch chain there is no "branch top
    // level" to be at. That is one reason tier 2 is weaker than tier 1, and
    // one reason `Split::Candidate` exists.
    let mut authorised: Option<u8> = None;
    let mut indices: Vec<u8> = Vec::new();
    let mut scan = cut;
    while scan < program.len() {
        let (next, op, data) = step(program, scan)?;
        if data.is_none() {
            match op {
                OP_AUTH_OUTPUT_COUNT
                    if scan >= 1
                        && program[scan - 1] == OP_ACTIVE_INPUT_INDEX
                        && authorised.is_none() =>
                {
                    let n = small_int(program, scan + 1)?;
                    if program.get(scan + 2..scan + 4)? != [OP_NUMEQUAL, OP_VERIFY] {
                        return None;
                    }
                    authorised = Some(n);
                }
                OP_AUTH_OUTPUT_IDX => {
                    if scan < 2 || program[scan - 2] != OP_ACTIVE_INPUT_INDEX {
                        return None;
                    }
                    indices.push(small_int(program, scan - 1)?);
                }
                _ => {}
            }
        }
        scan = next;
    }
    let authorised = authorised?;
    let mut sorted = indices.clone();
    sorted.sort_unstable();
    if sorted != (0..authorised).collect::<Vec<u8>>() {
        return None;
    }

    Some(Argent {
        split: Split::Candidate,
        generation: Generation::Fork,
        terminator: None,
        head_len: 0,
        state_len: cut,
        code_len: program.len() - cut,
        entrypoints: vec![Entrypoint {
            dispatch: Dispatch::Leaf,
            authorised: Authorised::Exact(authorised),
        }],
        template_hash: template_hash(&[], &program[cut..]),
        state_push_ends,
    })
}

/// Recognise an argent-compiled program, preferring the proven split.
///
/// `None` is the answer for every program kascov cannot show came out of
/// argent's leader codegen — including programs that carry the silverscript
/// dispatch envelope, which is most of the covenant chain.
pub fn recognize(program: &[u8]) -> Option<Argent> {
    recognize_dispatch(program).or_else(|| recognize_leaf(program))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The guard bytes are template bytes: moving the same state fields in or
    /// out of the guard is a different build and must hash differently. This
    /// is the property that lets one family be counted across instances.
    #[test]
    fn the_guard_is_part_of_the_template() {
        let a = template_hash(&[OP_TOALTSTACK], &[OP_FROMALTSTACK, 0x51]);
        let b = template_hash(&[], &[OP_FROMALTSTACK, 0x51]);
        assert_ne!(a, b);
    }

    /// The two arms hash the same halves to different numbers, so a build's
    /// generation must be read before its hash is trusted. Both keep the
    /// LE64 framing: `crate::kcc1::template_hash` is byte-identical to the
    /// 1.0 compiler's rule, `template_hash` to the fork's.
    #[test]
    fn the_two_arms_name_the_same_bytes_differently() {
        let prefix = [OP_TOALTSTACK];
        let suffix = [OP_FROMALTSTACK, 0x51];
        assert_ne!(
            Generation::Fork.template_hash(&prefix, &suffix),
            Generation::V1.template_hash(&prefix, &suffix)
        );
        assert_eq!(Generation::Fork.template_hash(&prefix, &suffix), template_hash(&prefix, &suffix));
        assert_eq!(
            Generation::V1.template_hash(&prefix, &suffix),
            crate::kcc1::template_hash(&prefix, &suffix)
        );
        assert_eq!((Generation::Fork.as_str(), Generation::V1.as_str()), ("fork", "1.0"));
    }

    #[test]
    fn step_refuses_a_truncated_push() {
        assert_eq!(step(&[0x02, 0xaa], 0), None);
        assert_eq!(step(&[OP_PUSHDATA1], 0), None);
        assert_eq!(step(&[OP_PUSHDATA2, 0x10, 0x00], 0), None);
        // OP_0 pushes the empty payload; OP_1..OP_16 are logic, not fields.
        assert_eq!(step(&[OP_0], 0), Some((1, OP_0, Some(0))));
        assert_eq!(step(&[OP_1], 0), Some((1, OP_1, None)));
    }

    #[test]
    fn small_int_covers_exactly_the_literal_opcodes() {
        assert_eq!(small_int(&[OP_0], 0), Some(0));
        assert_eq!(small_int(&[OP_1], 0), Some(1));
        assert_eq!(small_int(&[OP_16], 0), Some(16));
        assert_eq!(small_int(&[0x4f], 0), None); // OP_1NEGATE
        assert_eq!(small_int(&[0x01, 0x03], 0), None); // a data push of 3
        assert_eq!(small_int(&[], 0), None);
    }

    /// The three preamble forms, and what is not one of them.
    #[test]
    fn count_sites_read_exact_and_ranged_preambles() {
        // b9 cb 52 9c 69
        let exact = [OP_ACTIVE_INPUT_INDEX, OP_AUTH_OUTPUT_COUNT, 0x52, OP_NUMEQUAL, OP_VERIFY];
        assert_eq!(count_site(&exact, 1), Some(Authorised::Exact(2)));
        // b9 cb 52 94 76 51 a2 69 76 53 a1 69: two fixed handles, 1..=3 more
        let ranged = [
            OP_ACTIVE_INPUT_INDEX, OP_AUTH_OUTPUT_COUNT, 0x52, OP_SUB,
            OP_DUP, 0x51, OP_GREATERTHANOREQUAL, OP_VERIFY,
            OP_DUP, 0x53, OP_LESSTHANOREQUAL, OP_VERIFY,
        ];
        assert_eq!(count_site(&ranged, 1), Some(Authorised::Range { fixed: 2, min: 1, max: 3 }));
        // b9 cb 76 51 a2 69 76 53 a1 69: no fixed handle
        let bare = [
            OP_ACTIVE_INPUT_INDEX, OP_AUTH_OUTPUT_COUNT,
            OP_DUP, 0x51, OP_GREATERTHANOREQUAL, OP_VERIFY,
            OP_DUP, 0x53, OP_LESSTHANOREQUAL, OP_VERIFY,
        ];
        assert_eq!(count_site(&bare, 1), Some(Authorised::Range { fixed: 0, min: 1, max: 3 }));
        // b9 cb 51 94 cc: an operand (`count - 1`), not a site
        let operand = [OP_ACTIVE_INPUT_INDEX, OP_AUTH_OUTPUT_COUNT, 0x51, OP_SUB, OP_AUTH_OUTPUT_IDX];
        assert_eq!(count_site(&operand, 1), None);
        // bounds the wrong way round are nothing a compiler emits
        let inverted = [
            OP_ACTIVE_INPUT_INDEX, OP_AUTH_OUTPUT_COUNT,
            OP_DUP, 0x53, OP_GREATERTHANOREQUAL, OP_VERIFY,
            OP_DUP, 0x51, OP_LESSTHANOREQUAL, OP_VERIFY,
        ];
        assert_eq!(count_site(&inverted, 1), None);
    }

    #[test]
    fn empty_and_tiny_inputs_are_refused_without_panicking() {
        for program in [
            &[][..],
            &[OP_ENDIF][..],
            &[OP_TOALTSTACK][..],
            &[0x6a, 0x68][..],
            &[OP_TOALTSTACK, OP_FROMALTSTACK, OP_DUP, OP_DATA_4][..],
        ] {
            assert_eq!(recognize(program), None);
        }
    }

    #[test]
    fn a_state_window_is_a_run_of_pushes_and_nothing_else() {
        // push32 (all zero), push8 (the int 5 as NUM2BIN 8 writes it), OP_0
        let mut window = vec![0x20];
        window.extend_from_slice(&[0u8; 32]);
        window.extend_from_slice(&[0x08, 0x05, 0, 0, 0, 0, 0, 0, 0]);
        window.push(OP_0);
        let got = pushes(&window).expect("three pushes");
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].len(), 32);
        assert_eq!(script_num(got[1]), Some(5));
        assert!(got[2].is_empty());
        assert_eq!(script_num(got[2]), Some(0));
        // A logic byte inside the window is not state.
        let mut broken = window.clone();
        broken.push(OP_DUP);
        assert_eq!(pushes(&broken), None);
        // A push that runs past the end is refused, not read short.
        assert_eq!(pushes(&[0x05, 0xaa]), None);
        assert_eq!(pushes(&[]), None);
    }

    #[test]
    fn script_numbers_read_as_the_engine_reads_them() {
        assert_eq!(script_num(&[0x01]), Some(1));
        assert_eq!(script_num(&[0x81]), Some(-1));
        assert_eq!(script_num(&[0xff, 0x00]), Some(255));
        assert_eq!(script_num(&[0xff, 0x80]), Some(-255));
        assert_eq!(script_num(&[0, 0, 0, 0, 0, 0, 0, 0x40]), Some(1 << 62));
        assert_eq!(script_num(&[0; 9]), None, "nine bytes is not a script number");
    }

    /// The whole binding, on the fork-era argentc's own output: every
    /// contract's committed `template_hash_hex` reproduces from its
    /// `script_hex` and `state_span` under the BLAKE2b arm, which is the
    /// rule the deployed fork-era programs check and the rule `argent_builds`
    /// keys those families on. An uploaded artifact therefore names exactly
    /// the covenants whose reveals hash to it, with nothing taken on trust.
    #[test]
    fn fork_era_artifacts_commit_to_the_same_template_hash_the_chain_checks() {
        for (fixture, contracts) in [
            (
                include_str!("../fixtures/argent/tickets.artifact.json"),
                ["Issuer", "Ticket"],
            ),
            (
                include_str!("../fixtures/argent/icc_kcc20_asset.artifact.json"),
                ["KCC20", "MinterProxy"],
            ),
        ] {
            let doc: serde_json::Value = serde_json::from_str(fixture).unwrap();
            let listed: Vec<&str> = doc["sil_abi"]["contracts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["name"].as_str().unwrap())
                .collect();
            assert_eq!(listed, contracts);
            for contract in doc["sil_abi"]["contracts"].as_array().unwrap() {
                let script = hex::decode(contract["compiled"]["script_hex"].as_str().unwrap()).unwrap();
                let span = &contract["compiled"]["state_span"];
                let offset = span["offset"].as_u64().unwrap() as usize;
                let len = span["len"].as_u64().unwrap() as usize;
                let claimed = contract["compiled"]["template_hash_hex"].as_str().unwrap();
                let ours = template_hash(&script[..offset], &script[offset + len..]);
                assert_eq!(hex::encode(ours), claimed, "{}", contract["name"]);
                // Every example contract is a single-entry leaf, so kascov's
                // own recognizer lands on the same cut argentc declared, and
                // dates it as the fork.
                let seen = recognize(&script).expect("argentc output is argent");
                assert_eq!((seen.head_len, seen.state_len), (offset, len));
                assert_eq!(seen.template_hash, ours);
                assert_eq!(seen.generation, Generation::Fork);
                assert_eq!(seen.terminator, None);
                assert_eq!(seen.entrypoints[0].dispatch, Dispatch::Leaf);
                // …and the template state window is a run of pushes.
                assert!(pushes(&script[offset..offset + len]).is_some());
            }
        }
    }

    /// argent's security rule 5 (argentc e76ee07): a coordinated leader entry
    /// that consumes inputs restates its count against its covenant group's,
    /// `d2 b9 cb 9c 69`. Real builds carry it and are read; the same check
    /// written twice in one branch is not something the compiler emits, and
    /// the branch is left unread.
    #[test]
    fn the_rule_5_continuation_check_is_read_once_per_branch() {
        const SITE: [u8; 5] = [0xd2, 0xb9, 0xcb, 0x9c, 0x69];
        let doc: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/argent/open_icc_core.v1.artifact.json")).unwrap();
        let cell: Vec<u8> = doc["sil_abi"]["contracts"]["Cell"]["compiled"]["bytecode"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_u64().unwrap() as u8)
            .collect();
        let sites: Vec<usize> = cell.windows(5).enumerate().filter(|(_, w)| *w == SITE).map(|(i, _)| i).collect();
        assert!(!sites.is_empty(), "the e76ee07 Cell carries the rule-5 check");
        let seen = recognize(&cell).expect("a real e76ee07 build is read");
        assert_eq!(seen.generation, Generation::V1);
        assert_eq!(seen.entrypoint_count(), 2);
        // the check written a second time, right behind the first
        let mut doubled = cell.clone();
        doubled.splice(sites[0] + 5..sites[0] + 5, SITE);
        assert!(recognize(&doubled).is_none(), "a second rule-5 check in one branch is refused");
    }

    /// The same binding on SilverScript 1.0 output, argentc at 867b080 (pins
    /// silverscript v1.0.0) and at e76ee07 (the leader/delegate security
    /// rules): every contract carries the guard and tag rungs,
    /// hashes under BLAKE3 to exactly the `compiled.template_hash` argentc
    /// wrote, and declares as many rungs as the artifact declares entries,
    /// with the tags matching. The range fixture's Batch entry emits
    /// `Account[1..=3]` behind two fixed handles and is read as that range.
    #[test]
    fn v1_artifacts_commit_to_the_blake3_template_hash_the_program_checks() {
        for (fixture, contracts) in [
            (
                include_str!("../fixtures/argent/tickets.v1.artifact.json"),
                &["Issuer", "Ticket"][..],
            ),
            (
                include_str!("../fixtures/argent/icc_kcc20_asset.v1.artifact.json"),
                &["KCC20", "MinterProxy"][..],
            ),
            (
                include_str!("../fixtures/argent/entry_range_outputs.v1.artifact.json"),
                &["Account", "Batch"][..],
            ),
            (
                include_str!("../fixtures/argent/entry_range_only.v1.artifact.json"),
                &["Account", "Batch"][..],
            ),
            // argentc e76ee07: leader entries that consume inputs carry the
            // rule-5 continuation check (see `scan_branch`).
            (
                include_str!("../fixtures/argent/open_icc_core.v1.artifact.json"),
                &["Cell"][..],
            ),
            (
                include_str!("../fixtures/argent/stones.v1.artifact.json"),
                &["League", "Player", "StonesGame", "StonesSettle"][..],
            ),
            (
                include_str!("../fixtures/argent/entry_range_inputs.v1.artifact.json"),
                &["Account", "Batch"][..],
            ),
        ] {
            let doc: serde_json::Value = serde_json::from_str(fixture).unwrap();
            let map = doc["sil_abi"]["contracts"].as_object().unwrap();
            let listed: Vec<&str> = map.keys().map(String::as_str).collect();
            assert_eq!(listed, contracts);
            for (name, contract) in map {
                let script: Vec<u8> = contract["compiled"]["bytecode"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|b| b.as_u64().unwrap() as u8)
                    .collect();
                let claimed: Vec<u8> = contract["compiled"]["template_hash"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|b| b.as_u64().unwrap() as u8)
                    .collect();
                let span = &contract["compiled"]["state_span"];
                let offset = span["offset"].as_u64().unwrap() as usize;
                let len = span["len"].as_u64().unwrap() as usize;
                let seen = recognize(&script).unwrap_or_else(|| panic!("{name} is 1.0 argent"));
                assert_eq!(seen.generation, Generation::V1, "{name}");
                assert_eq!(seen.split, Split::Proven, "{name}");
                assert_eq!(seen.terminator, Some(Terminator::OpReturn), "{name}");
                assert_eq!((seen.head_len, seen.state_len), (offset, len), "{name}");
                assert_eq!(seen.template_hash.as_slice(), claimed.as_slice(), "{name}");
                assert_eq!(
                    seen.template_hash,
                    crate::kcc1::template_hash(&script[..offset], &script[offset + len..])
                );
                assert_ne!(
                    seen.template_hash,
                    template_hash(&script[..offset], &script[offset + len..]),
                    "{name}: the fork arm must not name a 1.0 build"
                );
                let entries = contract["entries"].as_object().unwrap();
                assert_eq!(seen.entrypoint_count(), entries.len(), "{name}");
                let mut declared: Vec<String> = entries
                    .values()
                    .map(|e| e["dispatch_tag"].as_str().unwrap().to_string())
                    .collect();
                declared.sort();
                let mut read: Vec<String> = seen
                    .entrypoints
                    .iter()
                    .map(|e| hex::encode(e.tag().expect("a 1.0 rung carries a tag")))
                    .collect();
                read.sort();
                assert_eq!(read, declared, "{name}");
                assert!(seen.entrypoints.iter().all(|e| e.selector().is_none()));
            }
        }
        let range: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/argent/entry_range_outputs.v1.artifact.json")).unwrap();
        let batch: Vec<u8> = range["sil_abi"]["contracts"]["Batch"]["compiled"]["bytecode"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_u64().unwrap() as u8)
            .collect();
        let seen = recognize(&batch).unwrap();
        assert_eq!(
            seen.entrypoints[0].authorised,
            Authorised::Range { fixed: 2, min: 1, max: 3 },
            "first and last are fixed, next is Account[1..=MAX_ACCOUNTS]"
        );
        assert_eq!(seen.entrypoints[0].authorised.exact(), None);
        let account: Vec<u8> = range["sil_abi"]["contracts"]["Account"]["compiled"]["bytecode"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_u64().unwrap() as u8)
            .collect();
        assert_eq!(recognize(&account).unwrap().entrypoints[0].authorised, Authorised::Exact(0));
        // No fixed handle at all: the emitter drops `- k`, and the range
        // still reads.
        let only: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/argent/entry_range_only.v1.artifact.json")).unwrap();
        let batch: Vec<u8> = only["sil_abi"]["contracts"]["Batch"]["compiled"]["bytecode"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_u64().unwrap() as u8)
            .collect();
        assert_eq!(
            recognize(&batch).unwrap().entrypoints[0].authorised,
            Authorised::Range { fixed: 0, min: 1, max: 3 }
        );
    }

    /// The KCC-20 reference app (argent-lang/kcc20-reference c8a0871): its
    /// three actors read as 1.0 argent with the artifact's own template
    /// hashes. KCC20's transfer emits `KCC20[1..=3]` and its borrowed path
    /// reads `next_outputs[0]` three conditionals deep, a member the range
    /// guarantees; a read one past that guarantee is still refused.
    #[test]
    fn the_kcc20_reference_actors_are_recognised() {
        for (program, hash, span, entries) in [
            (
                include_bytes!("../fixtures/kcc0020/reference_kcc20_a.bin").as_slice(),
                "9703112ee6e3555107cd168858992b77d3b74f655205b2b463b1f9ec2ec73cf7",
                145,
                &["79c71c23", "fd3ef14a"][..],
            ),
            (
                include_bytes!("../fixtures/kcc0020/reference_public_mint_a.bin").as_slice(),
                "14b0f06bc176b78c94f19292e409c622636f31dae6bb31ff3d4d3308a6053b49",
                150,
                &["0fe04b09", "52f03341", "35eecca6", "6f52c1a9", "ebe6a147"][..],
            ),
            (
                include_bytes!("../fixtures/kcc0020/reference_token_seed_a.bin").as_slice(),
                "f79a3fccf6f87d7076662410a17e54d1531889d9b60f630f9fbb8465a384e62d",
                132,
                &["d1f8dfa0", "5dc68e50", "f56af4df", "ebe6a147"][..],
            ),
        ] {
            let seen = recognize(program).unwrap_or_else(|| panic!("{hash} is 1.0 argent"));
            assert_eq!(seen.generation, Generation::V1);
            assert_eq!((seen.head_len, seen.state_len), (1, span));
            assert_eq!(hex::encode(seen.template_hash), hash);
            let mut read: Vec<String> = seen.entrypoints.iter().map(|e| hex::encode(e.tag().unwrap())).collect();
            let mut want: Vec<String> = entries.iter().map(|t| t.to_string()).collect();
            read.sort();
            want.sort();
            assert_eq!(read, want);
        }
        let kcc20 = include_bytes!("../fixtures/kcc0020/reference_kcc20_a.bin");
        let seen = recognize(kcc20).unwrap();
        assert_eq!(seen.entrypoints[0].authorised, Authorised::Range { fixed: 0, min: 1, max: 3 });
        assert_eq!(seen.entrypoints[1].authorised, Authorised::Exact(0));
        // the nested `b9 00 cc` of the borrowed path, rewritten to read
        // output 1, which a one-output transfer need not have
        let site = kcc20
            .windows(3)
            .position(|w| w == [OP_ACTIVE_INPUT_INDEX, OP_0, OP_AUTH_OUTPUT_IDX])
            .expect("the borrowed path reads next_outputs[0]");
        let mut past = kcc20.to_vec();
        past[site + 1] = OP_1;
        assert_eq!(recognize(&past), None);
    }

    /// Each 1.0 clause is load-bearing on the real Ticket program.
    #[test]
    fn each_v1_clause_is_load_bearing() {
        let doc: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/argent/tickets.v1.artifact.json")).unwrap();
        let good: Vec<u8> = doc["sil_abi"]["contracts"]["Ticket"]["compiled"]["bytecode"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_u64().unwrap() as u8)
            .collect();
        let seen = recognize(&good).expect("1.0 argent");
        let rung_at = 1 + seen.state_len + 1; // OP_DUP after the guard's close
        assert_eq!(good[rung_at], OP_DUP);
        assert_eq!(good[rung_at + 1], OP_DATA_4);
        // OpEqual becomes OpNumEqual: a tag compared as a number is neither
        // rung form.
        let mut wrong_compare = good.clone();
        wrong_compare[rung_at + 6] = OP_NUMEQUAL;
        assert_eq!(recognize(&wrong_compare), None);
        // A 1.0 chain on the older tail: `6a` becomes `75 00 69`.
        let mut old_tail = good.clone();
        let tail = old_tail.len() - 2; // … 67 6a 68
        assert_eq!(&old_tail[tail..], &[OP_RETURN, OP_ENDIF]);
        old_tail.splice(tail..tail + 1, [OP_DROP, OP_0, OP_VERIFY]);
        assert_eq!(dispatch_envelope(&old_tail), Some((1, Terminator::DropFail)));
        assert_eq!(recognize(&old_tail), None);
        // The guard is still template: the same tag chain hashed without it
        // is a different (and unrecognised) program.
        let mut no_guard = good.clone();
        no_guard[0] = OP_DROP;
        assert_eq!(recognize(&no_guard), None);
    }
}
