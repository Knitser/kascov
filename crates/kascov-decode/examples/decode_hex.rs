//! Dump the full opcode table (all 256 bytes) and, if given a hex arg,
//! disassemble it — for cross-checking the JS port.
use kascov_decode::disasm::{disassemble, opcode_info};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s == "--table").unwrap_or(false) {
        for op in 0u16..=255 {
            let (name, group) = opcode_info(op as u8);
            println!("{:02x} {} {:?}", op, name, group);
        }
        return;
    }
    if let Some(hex) = args.get(1) {
        let bytes = hex::decode(hex.trim()).expect("hex");
        let (ins, truncated) = disassemble(&bytes);
        println!("bytes {} instructions {} truncated {}", bytes.len(), ins.len(), truncated);
        for i in &ins {
            println!("{:04x}  {}", i.offset, i);
        }
    }
}
