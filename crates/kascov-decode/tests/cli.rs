//! The `kascov-decode` command, end to end, on real chain bytes: the recovery
//! fixtures are a testnet-10 program and the signature script of the spend that
//! revealed it (see fixtures/recovery/README).

use std::io::Write;
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_kascov-decode");
const PARENT: &[u8] = include_bytes!("../fixtures/recovery/kcom_2671_parent_56fc521e_0.bin");
const WITNESS: &[u8] = include_bytes!("../fixtures/recovery/kcom_witness_7d5f35f0.bin");
const PARENT_HASH: &str = "7cec521a9350d30408dae91466aeedc31fa0f34b43d30d6d23c06f650f333820";

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("run kascov-decode")
}

fn json(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("not JSON ({e}): {}", String::from_utf8_lossy(&out.stdout)))
}

#[test]
fn version_and_help() {
    let out = run(&["--version"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("kascov-decode "));
    let out = run(&["--help"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("kascov-decode check"));
}

#[test]
fn name_reads_a_program() {
    let out = run(&["name", &hex::encode(PARENT)]);
    assert!(out.status.success());
    let v = json(&out);
    assert_eq!(v["bytes"], 2671);
    assert_eq!(v["blake2b_256"], PARENT_HASH);
    assert_eq!(v["p2sh_script"], format!("aa20{PARENT_HASH}87"));
    assert_eq!(v["template"], "KaspaCom · token");
}

#[test]
fn hash_reads_standard_input_with_prefix_and_whitespace() {
    let mut child = Command::new(BIN)
        .args(["hash", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let hex = hex::encode(PARENT);
    let (a, b) = hex.split_at(hex.len() / 2);
    write!(child.stdin.take().unwrap(), "0x{a}\n  {b}\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert_eq!(json(&out)["blake2b_256"], PARENT_HASH);
}

#[test]
fn check_confirms_the_program_a_real_spend_revealed() {
    let spk = format!("aa20{PARENT_HASH}87");
    let out = run(&["check", &hex::encode(WITNESS), "--spk", &spk]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = json(&out);
    assert_eq!(v["spk_matches"], true);
    assert_eq!(v["push_only"], true);
    assert_eq!(v["program"]["bytes"], 2671);
    assert_eq!(v["program"]["blake2b_256"], PARENT_HASH);
    assert_eq!(v["program"]["template"], "KaspaCom · token");
}

#[test]
fn check_refuses_a_program_the_output_did_not_commit_to() {
    let wrong = format!("aa20{}87", "00".repeat(32));
    let out = run(&["check", &hex::encode(WITNESS), &format!("--spk={wrong}")]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(json(&out)["spk_matches"], false);
}

#[test]
fn bad_input_is_a_usage_error() {
    assert_eq!(run(&["name", "zz"]).status.code(), Some(2));
    assert_eq!(run(&["name"]).status.code(), Some(2));
    assert_eq!(run(&["frobnicate", "00"]).status.code(), Some(2));
    // an output script that is not P2SH
    let out = run(&["check", &hex::encode(WITNESS), "--spk", "51"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn disasm_lists_every_instruction() {
    let out = run(&["disasm", &hex::encode(PARENT)]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    let first = text.lines().next().unwrap();
    assert!(first.starts_with("2671 bytes, "), "{first}");
    assert!(text.lines().nth(1).unwrap().starts_with("0000  "));
}
