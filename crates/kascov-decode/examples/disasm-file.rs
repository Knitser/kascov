//! Disassemble a program file:
//!   cargo run -p kascov-decode --example disasm-file -- <path>
fn main() {
    let path = std::env::args().nth(1).expect("usage: disasm-file <path>");
    let bytes = std::fs::read(&path).expect("read program");
    let (instructions, truncated) = kascov_decode::disasm::disassemble(&bytes);
    println!("{} bytes, {} instructions, truncated tail: {truncated}", bytes.len(), instructions.len());
    for ins in &instructions {
        println!("{ins:?}");
    }
}
