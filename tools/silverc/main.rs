// SilverScript compiler CLI: compile a contract + constructor args to script hex.
// Usage: silverc <file.sil|-> [ctor_arg ...]     ('-' reads source from stdin)
use std::io::Read;
use debugger_session::args::parse_ctor_args;
use silverscript_lang::ast::parse_contract_ast;
use silverscript_lang::compiler::{compile_contract, CompileOptions};

fn main() {
    let mut argv = std::env::args().skip(1);
    let path = argv.next().expect("usage: silverc <file.sil|-> [ctor args...]");
    let raw_args: Vec<String> = argv.collect();
    let source = if path == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s).expect("read stdin");
        s
    } else {
        std::fs::read_to_string(&path).expect("read source")
    };
    let contract = match parse_contract_ast(&source) {
        Ok(c) => c,
        Err(e) => { eprintln!("parse error: {e:?}"); std::process::exit(1); }
    };
    let ctor = match parse_ctor_args(&contract, &raw_args) {
        Ok(a) => a,
        Err(e) => { eprintln!("arg error: {e}"); std::process::exit(1); }
    };
    match compile_contract(&source, &ctor, CompileOptions::default()) {
        Ok(c) => println!("{}", c.script.iter().map(|b| format!("{:02x}", b)).collect::<String>()),
        Err(e) => { eprintln!("compile error: {e:?}"); std::process::exit(1); }
    }
}
