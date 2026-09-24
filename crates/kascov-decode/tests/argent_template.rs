//! The argent template-hash commitment, proven against real testnet-10 bytes.
//!
//! Zealous Swap's 8,643 B market carries a 32-byte constant in its state and a
//! 905 B blob in its witness. The blob is the immutable code half of the
//! order-cell template; the constant is its commitment. If our hash function
//! ever drifts from argent's, this test fails and kascov stops being able to
//! verify any argent app on chain.
//!
//! Bytes are the real ones, from the spend of covenant
//! 35d0f746e66010eedadad11e… on testnet-10.

use kascov_decode::argent::template_hash;

fn suffix() -> Vec<u8> {
    let hex = include_str!("fixtures/argent-order-cell-suffix.hex");
    let hex = hex.trim();
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("fixture is hex"))
        .collect()
}

#[test]
fn reproduces_a_real_zealous_commitment() {
    let code = suffix();
    assert_eq!(code.len(), 905, "the order-cell code half is 905 bytes");

    // An empty head: this template is all code, with no leading state framing.
    let got = template_hash(&[], &code);
    let want =
        hex_lit("49470b2c17f3cb3cf782c1dc721053ba858366c5f15361147ae884826847f85d");
    assert_eq!(got, want, "template hash must match the on-chain commitment");
}

#[test]
fn the_length_framing_is_load_bearing() {
    // The framing is what stops a prefix/suffix split from being ambiguous:
    // without the lengths, moving one byte from the head to the code would
    // leave the hash unchanged and two different templates would collide.
    let a = template_hash(b"\x6b", b"\x01\x02");
    let b = template_hash(b"\x6b\x01", b"\x02");
    assert_ne!(a, b, "a re-split must not produce the same commitment");
}

fn hex_lit(s: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).expect("hex");
    }
    out
}
