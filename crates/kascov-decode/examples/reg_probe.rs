use kascov_decode::{kcc20, p2sh_hash, Registry};
fn main() {
    let reg = Registry::default();
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path).expect("read");
        let d = reg.decode(0, &bytes);
        let digest = blake2b_simd::Params::new().hash_length(32).hash(&bytes);
        println!("== {} len={} blake2b={}", path, bytes.len(), hex::encode(digest.as_bytes()));
        println!("   decoder={} template={:?} fields={}", d.decoder, d.template, d.fields.len());
        for f in &d.fields {
            let v = if f.value.len() > 40 { format!("{}…({}B)", hex::encode(&f.value[..40]), f.value.len()) } else { hex::encode(&f.value) };
            println!("     - {} = {}", f.name, v);
        }
        println!("   kcc20.revealed_template = {:?}", kcc20::revealed_template(&reg, 0, &bytes));
        match kcc20::locate_state_block(&bytes) {
            Some(x) => println!("   kcc20.locate_state_block = {:?}", x),
            None => println!("   kcc20.locate_state_block = None"),
        }
        match kcc20::decode_token_state(&reg, 0, &bytes) {
            Some(s) => println!("   token_state owner={} type={} amount={:?} minter={:?}", hex::encode(s.owner), s.identifier_type, s.amount_i64(), s.is_minter()),
            None => println!("   token_state = None"),
        }
        let _ = p2sh_hash;
    }
}
