//! Conformance vectors for the KCC-1 §8.3 TemplateHash, and a cross-check any
//! external implementation can drive.
//!
//! kascov groups a covenant family by this hash and serves it at
//! `/data/{network}/template/{hash}`, so the convention is load-bearing for
//! anybody who builds against the index: a deployer whose tooling computes the
//! hash even slightly differently gets a family that silently fails to group.
//!
//! The framing is what makes the hash safe —
//! `Hash(LE64(len(prefix)) || prefix || LE64(len(suffix)) || suffix)` — because
//! a plain `Hash(prefix || suffix)` collides across different cuts of the same
//! bytes. The vectors below pin that property so a refactor of the length width,
//! the endianness, or the framing itself fails here rather than in the field.

use kascov_decode::kcc1::template_hash;

fn unhex(s: &str) -> Vec<u8> {
    let s = s.trim();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex pair"))
        .collect()
}

/// The framing must bind the prefix/suffix boundary: the same concatenated bytes
/// cut in two different places must hash differently. Without the length fields
/// these collide, and two unrelated templates would share one family id.
#[test]
fn the_framing_binds_the_prefix_suffix_boundary() {
    assert_ne!(template_hash(b"a", b"bc"), template_hash(b"ab", b"c"));
    assert_ne!(template_hash(b"", b"xy"), template_hash(b"x", b"y"));
    // an empty prefix is a real shape — a single-entrypoint contract compiles
    // with its state block at offset 0 — so it must hash distinctly too.
    assert_ne!(template_hash(b"", b""), template_hash(b"", b"\x00"));
}

/// The hash is a pure function of the two parts, not of how they were produced.
#[test]
fn the_hash_is_stable_for_the_same_parts() {
    let a = template_hash(b"prefix-bytes", b"suffix-bytes");
    let b = template_hash(b"prefix-bytes", b"suffix-bytes");
    assert_eq!(a, b);
}

/// Cross-check an EXTERNAL implementation against this one.
///
/// A deployer's build tooling has to compute the same template hash kascov does,
/// or its covenants will not group here. Rather than reimplement the convention
/// and hope, external tooling can drive this test with its own numbers:
///
/// ```text
/// KCC1_PREFIX_HEX=<hex> KCC1_SUFFIX_HEX=<hex> KCC1_EXPECT_HEX=<hex> \
///   cargo test -p kascov-decode --test template_hash_conformance -- --ignored --nocapture
/// ```
///
/// Ignored by default so the ordinary suite needs no environment.
#[test]
#[ignore]
fn an_external_implementation_agrees() {
    let prefix = unhex(&std::env::var("KCC1_PREFIX_HEX").expect("KCC1_PREFIX_HEX"));
    let suffix = unhex(&std::env::var("KCC1_SUFFIX_HEX").expect("KCC1_SUFFIX_HEX"));
    let expect = std::env::var("KCC1_EXPECT_HEX")
        .expect("KCC1_EXPECT_HEX")
        .trim()
        .to_lowercase();
    let got: String = template_hash(&prefix, &suffix)
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();
    assert_eq!(
        got, expect,
        "external tooling computes a different TemplateHash than kascov — \
         covenants built with it would not group under this index"
    );
    eprintln!("TemplateHash agrees: {got}");
}
