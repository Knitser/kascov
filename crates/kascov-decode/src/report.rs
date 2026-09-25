//! The answers the `kascov-decode` command gives, as values, so that every
//! front end (the command, the WebAssembly build) says the same thing about
//! the same bytes. Nothing here reads anything but its arguments.

use std::fmt;
use std::sync::OnceLock;

use serde_json::{json, Value};

use crate::{argent, disasm::disassemble, p2sh_hash, p2sh_reveal, Registry};

/// The default registry, built once: deriving its skeletons is the costly part.
fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::default)
}

fn blake2b_256(bytes: &[u8]) -> String {
    hex::encode(
        blake2b_simd::Params::new()
            .hash_length(32)
            .hash(bytes)
            .as_bytes(),
    )
}

/// Why some text is not a byte string.
#[derive(Debug)]
pub enum HexError {
    Empty,
    NotHex(hex::FromHexError),
}

impl fmt::Display for HexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HexError::Empty => f.write_str("empty"),
            HexError::NotHex(e) => write!(f, "not hex: {e}"),
        }
    }
}

/// Bytes from hex the way people paste it: a `0x` prefix and any whitespace,
/// line breaks included, are ignored.
pub fn parse_hex(text: &str) -> Result<Vec<u8>, HexError> {
    let compact: String = text.split_whitespace().collect();
    let digits = compact
        .strip_prefix("0x")
        .or_else(|| compact.strip_prefix("0X"))
        .unwrap_or(&compact);
    if digits.is_empty() {
        return Err(HexError::Empty);
    }
    hex::decode(digits).map_err(HexError::NotHex)
}

/// What kascov makes of a program: the registry's verdict, plus the argent
/// recognizer's reading when the program is argent-compiled.
pub fn name(program: &[u8]) -> Value {
    let decoded = registry().decode(0, program);
    let digest = blake2b_256(program);
    let mut out = json!({
        "bytes": program.len(),
        "p2sh_script": format!("aa20{digest}87"),
        "blake2b_256": digest,
        "decoder": decoded.decoder,
        "template": decoded.template,
        "generation": decoded.generation,
        "fields": decoded.fields,
        "uses_covenant_ops": decoded.uses_covenant_ops,
    });
    if let Some(a) = argent::recognize(program) {
        out["argent"] = json!({
            "generation": a.generation.as_str(),
            "entrypoints": a.entrypoint_count(),
            "template_hash": hex::encode(a.template_hash),
            "template_hash_rule": a.generation.hash_name(),
        });
    }
    out
}

/// A program's BLAKE2b-256 and the P2SH script public key that commits to it.
pub fn hash(program: &[u8]) -> Value {
    let digest = blake2b_256(program);
    json!({
        "bytes": program.len(),
        "p2sh_script": format!("aa20{digest}87"),
        "blake2b_256": digest,
    })
}

/// The opcode listing: a summary line, then one line per instruction with
/// its offset.
pub fn disasm(program: &[u8]) -> String {
    let (instructions, truncated) = disassemble(program);
    let mut out = format!(
        "{} bytes, {} instructions{}",
        program.len(),
        instructions.len(),
        if truncated {
            ", truncated: the last push runs past the end"
        } else {
            ""
        }
    );
    for i in &instructions {
        out.push_str(&format!("\n{:04x}  {i}", i.offset));
    }
    out
}

/// Why [`check`] could not read what it was given.
#[derive(Debug)]
pub enum CheckError {
    /// The script public key is not `aa20 <32-byte hash> 87`.
    NotP2sh,
    /// The signature script's last push runs past its end.
    EndsInsidePush,
    /// The signature script does not end with a push, so it reveals nothing.
    NoFinalPush,
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            CheckError::NotP2sh => "the script public key is not P2SH (aa20 <32-byte hash> 87)",
            CheckError::EndsInsidePush => "the signature script ends inside a push",
            CheckError::NoFinalPush => {
                "the signature script does not end with a push, so it reveals no program"
            }
        })
    }
}

/// Take the program a spending input revealed (the last push of its
/// signature script), check it against the output it spends when that
/// output's script public key is given, and name it. `spk_matches` is
/// `false` when the program does not hash to the output's commitment, and
/// `null` when no script public key was given.
pub fn check(signature_script: &[u8], spk: Option<&[u8]>) -> Result<Value, CheckError> {
    if spk.is_some_and(|spk| p2sh_hash(spk).is_none()) {
        return Err(CheckError::NotP2sh);
    }
    let (instructions, truncated) = disassemble(signature_script);
    if truncated {
        return Err(CheckError::EndsInsidePush);
    }
    let program = instructions
        .last()
        .and_then(|i| i.data.clone())
        .ok_or(CheckError::NoFinalPush)?;
    // Push-only as consensus reads it: data pushes (0x00-0x4e), OP_1NEGATE
    // (0x4f) and OP_1..OP_16 (0x51-0x60). OP_RESERVED (0x50) is not a push.
    let push_only = instructions
        .iter()
        .all(|i| i.opcode <= 0x60 && i.opcode != 0x50);
    // The same test kascov runs on every spend: the final push must hash to
    // the commitment of the output being spent.
    let matches = spk.map(|spk| p2sh_reveal(spk, signature_script).is_some());
    Ok(json!({
        "signature_script_bytes": signature_script.len(),
        "pushes": instructions.len(),
        "push_only": push_only,
        "spk_matches": matches,
        "program": name(&program),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_is_read_the_way_people_paste_it() {
        assert_eq!(parse_hex("0xAB cd\n 01").unwrap(), [0xab, 0xcd, 0x01]);
        assert_eq!(parse_hex("0X00").unwrap(), [0x00]);
        assert!(matches!(parse_hex(" \n"), Err(HexError::Empty)));
        assert!(matches!(parse_hex("0x"), Err(HexError::Empty)));
        assert!(matches!(parse_hex("abc"), Err(HexError::NotHex(_))));
        assert!(matches!(parse_hex("zz"), Err(HexError::NotHex(_))));
    }

    #[test]
    fn check_needs_a_final_push_and_a_p2sh_output() {
        // OP_1 OP_ADD: ends with an opcode, not a push
        assert!(matches!(
            check(&[0x51, 0x93], None),
            Err(CheckError::NoFinalPush)
        ));
        // a 3-byte push with one byte present
        assert!(matches!(
            check(&[0x03, 0xaa], None),
            Err(CheckError::EndsInsidePush)
        ));
        assert!(matches!(
            check(&[0x01, 0x51], Some(&[0x51][..])),
            Err(CheckError::NotP2sh)
        ));
    }

    #[test]
    fn check_matches_only_the_committed_program() {
        let program = [0x51u8];
        let sig = [0x01, 0x51];
        let committed = hex::decode(hash(&program)["p2sh_script"].as_str().unwrap()).unwrap();
        let v = check(&sig, Some(committed.as_slice())).unwrap();
        assert_eq!(v["spk_matches"], true);
        assert_eq!(v["push_only"], true);
        assert_eq!(v["program"]["bytes"], 1);

        let other = hex::decode(format!("aa20{}87", "00".repeat(32))).unwrap();
        assert_eq!(
            check(&sig, Some(other.as_slice())).unwrap()["spk_matches"],
            false
        );
        assert_eq!(check(&sig, None).unwrap()["spk_matches"], Value::Null);
    }

    #[test]
    fn disasm_lists_offsets() {
        let text = disasm(&[0x51, 0x93]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "2 bytes, 2 instructions");
        assert!(lines[1].starts_with("0000  "));
        assert!(lines[2].starts_with("0001  "));
        assert!(disasm(&[0x03, 0xaa])
            .lines()
            .next()
            .unwrap()
            .ends_with("past the end"));
    }
}
