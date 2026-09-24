//! The embedded SilverScript dumps, reproduced from the compilers they name.
//!
//! Every skeleton in `silverscript_skeletons()` was cut from two builds of
//! one example contract by one compiler generation. This drives each
//! generation's real `silverc` over the source text the playground ships
//! (web/gen.js, mirrored byte for byte under fixtures/silverscript) with the
//! registry's dump-A sentinel arguments, and asserts two things: the bytes
//! decode to the family name with that generation stamped, and they ARE the
//! embedded dump, so the skeleton describes exactly what the compiler emits
//! and nothing that was hand-edited.
//!
//! The binaries are local builds, named by environment variable; a
//! generation whose variable is unset or points nowhere is skipped with a
//! message, never faked:
//!
//!   KASCOV_SILVERC_D25BD34_BIN  tools/silverc over kaspanet d25bd34 (stdin wrapper)
//!   KASCOV_SILVERC_FORK_BIN     tools/silverc over michaelsutton d57e5dff (stdin wrapper)
//!   KASCOV_SILVERC_V1_BIN       kaspanet/silverscript v1.0.0 (3ed9733) `silverc` CLI

use std::path::Path;
use std::process::{Command, Stdio};

use kascov_decode::{
    silverscript_sentinels, silverscript_skeletons, Registry, SILVERSCRIPT_DUMPS,
    SILVERSCRIPT_GEN_D25BD34, SILVERSCRIPT_GEN_FORK, SILVERSCRIPT_GEN_V1,
};

/// (generation, env var, fixture directory, contract)
const GENERATIONS: &[(&str, &str, &str, Contract)] = &[
    (SILVERSCRIPT_GEN_D25BD34, "KASCOV_SILVERC_D25BD34_BIN", "fork", Contract::StdinWrapper),
    (SILVERSCRIPT_GEN_FORK, "KASCOV_SILVERC_FORK_BIN", "fork", Contract::StdinWrapper),
    (SILVERSCRIPT_GEN_V1, "KASCOV_SILVERC_V1_BIN", "v1", Contract::UpstreamCli),
];

#[derive(Clone, Copy)]
enum Contract {
    /// `silverc - <arg>...`, source on stdin, hex on stdout.
    StdinWrapper,
    /// `silverc <file> --constructor-args <json> --stdout`, a SilAbiArtifact.
    UpstreamCli,
}

const FAMILIES: &[(&str, &str)] = &[
    ("SilverScript · Mecenas", "mecenas.sil"),
    ("SilverScript · Escrow", "escrow.sil"),
    ("SilverScript · LastWill", "lastwill.sil"),
];

fn fixture(dir: &str, file: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/silverscript")
        .join(dir)
        .join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn local_bin(var: &str) -> Option<String> {
    let path = std::env::var(var).ok()?;
    if Path::new(&path).is_file() {
        Some(path)
    } else {
        eprintln!("{var}={path} is not a file; skipping");
        None
    }
}

/// A sentinel value as the bare positional string the wrapper takes: hex
/// for 32-byte values, a decimal for script-number ints.
fn bare_arg(value: &[u8]) -> String {
    if value.len() == 32 {
        format!("0x{}", hex::encode(value))
    } else {
        snum_decode(value).to_string()
    }
}

/// The same value as the 1.0 portable ABI JSON element.
fn abi_arg(value: &[u8]) -> serde_json::Value {
    if value.len() == 32 {
        serde_json::json!({ "kind": "bytes", "value": value })
    } else {
        serde_json::json!({ "kind": "int", "value": snum_decode(value) })
    }
}

fn snum_decode(bytes: &[u8]) -> i64 {
    let mut n: i64 = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let b = if i + 1 == bytes.len() { b & 0x7f } else { b };
        n |= (b as i64) << (8 * i);
    }
    if bytes.last().is_some_and(|b| b & 0x80 != 0) {
        -n
    } else {
        n
    }
}

fn compile(bin: &str, contract: Contract, source: &str, args: &[(&str, Vec<u8>)]) -> Vec<u8> {
    match contract {
        Contract::StdinWrapper => {
            use std::io::Write;
            let mut child = Command::new(bin)
                .arg("-")
                .args(args.iter().map(|(_, v)| bare_arg(v)))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn silverc");
            child.stdin.take().unwrap().write_all(source.as_bytes()).unwrap();
            let out = child.wait_with_output().unwrap();
            assert!(out.status.success(), "silverc: {}", String::from_utf8_lossy(&out.stderr));
            hex::decode(String::from_utf8_lossy(&out.stdout).trim()).expect("wrapper prints hex")
        }
        Contract::UpstreamCli => {
            let dir = std::env::temp_dir().join(format!(
                "kascov-decode-silverc-{}-{}",
                std::process::id(),
                args.len()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let src = dir.join("contract.sil");
            std::fs::write(&src, source).unwrap();
            let ctor = dir.join("args.json");
            let values: Vec<serde_json::Value> = args.iter().map(|(_, v)| abi_arg(v)).collect();
            std::fs::write(&ctor, serde_json::Value::Array(values).to_string()).unwrap();
            let out = Command::new(bin)
                .arg(&src)
                .arg("--constructor-args")
                .arg(&ctor)
                .arg("--stdout")
                .output()
                .expect("spawn silverc");
            let _ = std::fs::remove_dir_all(&dir);
            assert!(out.status.success(), "silverc: {}", String::from_utf8_lossy(&out.stderr));
            let artifact: serde_json::Value = serde_json::from_slice(&out.stdout).expect("artifact json");
            let (_, contract) = artifact["contracts"].as_object().unwrap().iter().next().unwrap();
            contract["compiled"]["bytecode"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| b.as_u64().unwrap() as u8)
                .collect()
        }
    }
}

#[test]
fn each_generation_compiles_the_playground_sources_to_its_registered_dumps() {
    let registry = Registry::default();
    let skeletons = silverscript_skeletons();
    let mut proven = 0;
    for (generation, var, dir, contract) in GENERATIONS {
        let Some(bin) = local_bin(var) else {
            eprintln!("skipped {generation}: {var} unset");
            continue;
        };
        for (name, file) in FAMILIES {
            let source = fixture(dir, file);
            let sentinels = silverscript_sentinels(name).expect("family sentinels");
            let bytes = compile(&bin, *contract, &source, &sentinels);

            let decoded = registry.decode(0, &bytes);
            assert_eq!(decoded.template, Some(*name), "{generation} {file}");
            assert_eq!(decoded.generation, Some(*generation), "{generation} {file}");

            let (_, _, dump_a, _) = SILVERSCRIPT_DUMPS
                .iter()
                .find(|(n, g, _, _)| n == name && g == generation)
                .expect("registered dump");
            assert_eq!(hex::encode(&bytes), *dump_a, "{generation} {file}: compiler output is not the embedded dump");

            let skeleton = skeletons
                .iter()
                .find(|s| s.name == *name && s.generation == Some(*generation))
                .unwrap();
            let args: Vec<(&str, &[u8])> = sentinels.iter().map(|(l, v)| (*l, v.as_slice())).collect();
            assert_eq!(skeleton.emit(&args).unwrap(), bytes, "{generation} {file}: emit != compiler");
            proven += 1;
        }
        eprintln!("proved {generation} against {bin}");
    }
    eprintln!("{proven} (generation, family) pairs reproduced from a local compiler");
}
