//! `kascov-decode`: name, disassemble and check Kaspa covenant programs from
//! their bytes, with the same code kascov.io runs. It reads nothing but the
//! bytes it is given: no node, no network, no database.
//!
//! ```text
//! kascov-decode name   <program hex | ->
//! kascov-decode disasm <program hex | ->
//! kascov-decode hash   <program hex | ->
//! kascov-decode check  <signature script hex | -> [--spk <script public key hex>]
//! ```
//!
//! `-` reads the hex from standard input. Hex may carry a `0x` prefix and
//! whitespace. Exit status: 0 on success, 1 when `check` finds that the
//! revealed program does not match the output it spends, 2 on bad input.

use std::io::{Read, Write};
use std::process::ExitCode;

use kascov_decode::report;
use serde_json::Value;

const USAGE: &str = "\
kascov-decode: name, disassemble and check Kaspa covenant programs from their bytes

USAGE:
  kascov-decode name   <program hex | ->   what the program is, as JSON
  kascov-decode disasm <program hex | ->   the opcode listing
  kascov-decode hash   <program hex | ->   its BLAKE2b-256 and the P2SH script committing to it
  kascov-decode check  <signature script hex | -> [--spk <script public key hex>]
                                           take the program a spending input revealed (the
                                           last push of its signature script), check it against
                                           the output it spends when --spk is given, and name it
  kascov-decode --version

`-` reads the hex from standard input.";

enum Failure {
    /// Bad arguments or input: exit 2.
    Usage(String),
    /// The input was fine and the answer is no: exit 1.
    Check,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Check) => ExitCode::from(1),
        Err(Failure::Usage(msg)) => {
            eprintln!("kascov-decode: {msg}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &[String]) -> Result<(), Failure> {
    let Some((command, rest)) = args.split_first() else {
        return Err(Failure::Usage("no command given".into()));
    };
    match command.as_str() {
        "-h" | "--help" | "help" => emit(USAGE),
        "-V" | "--version" => emit(&format!("kascov-decode {}", env!("CARGO_PKG_VERSION"))),
        "name" => print_json(&report::name(&only_input(rest)?)),
        "disasm" => emit(&report::disasm(&only_input(rest)?)),
        "hash" => print_json(&report::hash(&only_input(rest)?)),
        "check" => return check(rest),
        other => return Err(Failure::Usage(format!("unknown command `{other}`"))),
    }
    Ok(())
}

fn check(rest: &[String]) -> Result<(), Failure> {
    let mut spk_arg: Option<&str> = None;
    let mut positional: Vec<&str> = Vec::new();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        if a == "--spk" {
            spk_arg = Some(
                it.next()
                    .ok_or_else(|| Failure::Usage("--spk needs a value".into()))?,
            );
        } else if let Some(v) = a.strip_prefix("--spk=") {
            spk_arg = Some(v);
        } else {
            positional.push(a);
        }
    }
    let [sig_arg] = positional[..] else {
        return Err(Failure::Usage("check takes one signature script".into()));
    };
    let sig = read_hex(sig_arg, "signature script")?;
    let spk = spk_arg
        .map(|s| read_hex(s, "script public key"))
        .transpose()?;
    let answer = report::check(&sig, spk.as_deref()).map_err(|e| Failure::Usage(e.to_string()))?;
    print_json(&answer);
    match answer["spk_matches"] {
        Value::Bool(false) => Err(Failure::Check),
        _ => Ok(()),
    }
}

fn only_input(rest: &[String]) -> Result<Vec<u8>, Failure> {
    match rest {
        [one] => read_hex(one, "program"),
        [] => Err(Failure::Usage(
            "give the program as hex, or `-` to read it from standard input".into(),
        )),
        _ => Err(Failure::Usage("expected exactly one program".into())),
    }
}

/// Hex from the argument, or from standard input for `-`.
fn read_hex(arg: &str, what: &str) -> Result<Vec<u8>, Failure> {
    let text = if arg == "-" {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| Failure::Usage(format!("could not read standard input: {e}")))?;
        s
    } else {
        arg.to_owned()
    };
    report::parse_hex(&text).map_err(|e| Failure::Usage(format!("the {what} is {e}")))
}

fn print_json(v: &Value) {
    emit(&serde_json::to_string_pretty(v).expect("json"));
}

/// Write to standard output. A reader that stops early (`| head`) closes the
/// pipe; that ends the output quietly instead of panicking.
fn emit(text: &str) {
    let mut out = std::io::stdout().lock();
    if let Err(e) = writeln!(out, "{text}") {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
        eprintln!("kascov-decode: could not write output: {e}");
        std::process::exit(2);
    }
}
