//! kascov-decode as a WebAssembly module. `js/kascov-decode.mjs` loads it and
//! gives JavaScript the four answers of the `kascov-decode` command: name,
//! hash, disasm and check. Each takes hex text and answers exactly what the
//! command answers, because both call `kascov_decode::report`.
//!
//! The boundary is plain memory, so the module imports nothing and needs no
//! generated glue. JavaScript copies its UTF-8 input into a buffer from
//! `kd_alloc`, calls an export, and gets back a pointer to 4 bytes of
//! little-endian length followed by that many bytes of JSON: `{"ok": …}` or
//! `{"error": "…"}`. It reads that and hands the buffer back to `kd_free`,
//! length prefix included.

use kascov_decode::report;
use serde_json::{json, Value};

/// A zeroed buffer of `len` bytes for JavaScript to write its input into.
#[no_mangle]
pub extern "C" fn kd_alloc(len: usize) -> *mut u8 {
    Box::into_raw(vec![0u8; len].into_boxed_slice()).cast()
}

/// Free a buffer from `kd_alloc`, or an answer (its length prefix included).
///
/// # Safety
/// `ptr` and `len` must describe one such buffer, freed once.
#[no_mangle]
pub unsafe extern "C" fn kd_free(ptr: *mut u8, len: usize) {
    drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)));
}

/// The UTF-8 text JavaScript wrote at `ptr`; empty when `len` is 0.
///
/// # Safety
/// `ptr` must point at `len` readable bytes when `len` is not 0.
unsafe fn text<'a>(ptr: *const u8, len: usize) -> Result<&'a str, String> {
    if len == 0 {
        return Ok("");
    }
    std::str::from_utf8(std::slice::from_raw_parts(ptr, len))
        .map_err(|_| "the input is not UTF-8".to_string())
}

fn bytes(hex: &str, what: &str) -> Result<Vec<u8>, String> {
    report::parse_hex(hex).map_err(|e| format!("the {what} is {e}"))
}

pub fn name(program_hex: &str) -> Result<Value, String> {
    Ok(report::name(&bytes(program_hex, "program")?))
}

pub fn hash(program_hex: &str) -> Result<Value, String> {
    Ok(report::hash(&bytes(program_hex, "program")?))
}

pub fn disasm(program_hex: &str) -> Result<Value, String> {
    Ok(Value::String(report::disasm(&bytes(
        program_hex,
        "program",
    )?)))
}

/// `spk_hex` empty means no script public key was given.
pub fn check(signature_script_hex: &str, spk_hex: &str) -> Result<Value, String> {
    let sig = bytes(signature_script_hex, "signature script")?;
    let spk = if spk_hex.is_empty() {
        None
    } else {
        Some(bytes(spk_hex, "script public key")?)
    };
    report::check(&sig, spk.as_deref()).map_err(|e| e.to_string())
}

/// The answer JavaScript reads: a 4-byte little-endian length, then the JSON.
fn answer(result: Result<Value, String>) -> *mut u8 {
    let envelope = match result {
        Ok(v) => json!({ "ok": v }),
        Err(e) => json!({ "error": e }),
    };
    let body = serde_json::to_vec(&envelope).expect("json");
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    Box::into_raw(out.into_boxed_slice()).cast()
}

/// # Safety
/// `ptr` must point at `len` bytes from `kd_alloc`.
#[no_mangle]
pub unsafe extern "C" fn kd_name(ptr: *const u8, len: usize) -> *mut u8 {
    answer(text(ptr, len).and_then(name))
}

/// # Safety
/// `ptr` must point at `len` bytes from `kd_alloc`.
#[no_mangle]
pub unsafe extern "C" fn kd_hash(ptr: *const u8, len: usize) -> *mut u8 {
    answer(text(ptr, len).and_then(hash))
}

/// # Safety
/// `ptr` must point at `len` bytes from `kd_alloc`.
#[no_mangle]
pub unsafe extern "C" fn kd_disasm(ptr: *const u8, len: usize) -> *mut u8 {
    answer(text(ptr, len).and_then(disasm))
}

/// # Safety
/// Each pointer must point at its length's bytes from `kd_alloc`; a length
/// of 0 needs no buffer.
#[no_mangle]
pub unsafe extern "C" fn kd_check(
    sig: *const u8,
    sig_len: usize,
    spk: *const u8,
    spk_len: usize,
) -> *mut u8 {
    answer(text(sig, sig_len).and_then(|sig| check(sig, text(spk, spk_len)?)))
}

#[no_mangle]
pub extern "C" fn kd_version() -> *mut u8 {
    answer(Ok(Value::String(env!("CARGO_PKG_VERSION").to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_carry_their_length() {
        let ptr = kd_version();
        let len = unsafe { u32::from_le_bytes(*ptr.cast::<[u8; 4]>()) } as usize;
        let body = unsafe { std::slice::from_raw_parts(ptr.add(4), len) };
        let v: Value = serde_json::from_slice(body).unwrap();
        assert_eq!(v["ok"], env!("CARGO_PKG_VERSION"));
        unsafe { kd_free(ptr, 4 + len) };
    }

    #[test]
    fn bad_input_is_an_error_not_a_trap() {
        assert_eq!(
            name("zz").unwrap_err().split(':').next(),
            Some("the program is not hex")
        );
        assert_eq!(name("").unwrap_err(), "the program is empty");
        // OP_1 OP_ADD ends with an opcode, so it reveals nothing
        assert!(check("5193", "")
            .unwrap_err()
            .contains("does not end with a push"));
        assert!(check("0151", "51").unwrap_err().contains("not P2SH"));
    }

    #[test]
    fn check_without_a_script_public_key_names_the_program() {
        let v = check("0151", "").unwrap();
        assert_eq!(v["spk_matches"], Value::Null);
        assert_eq!(v["program"]["bytes"], 1);
    }
}
