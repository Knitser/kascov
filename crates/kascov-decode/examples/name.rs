//! Name a covenant program from its bytes, the way kascov.io does:
//!
//! ```text
//! cargo run --example name -- <program hex>
//! ```
//!
//! Prints the registry's verdict as JSON: the decoder that matched, the
//! template name, the compiler generation when a skeleton proves one, and the
//! labelled fields. When the program is argent-compiled it adds the argent
//! recognizer's reading. Nothing is read but the bytes given: no node, no
//! network, no database.
use kascov_decode::{argent, Registry};

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: cargo run --example name -- <program hex>");
        std::process::exit(2);
    });
    let bytes = hex::decode(arg.trim().trim_start_matches("0x")).unwrap_or_else(|e| {
        eprintln!("not hex: {e}");
        std::process::exit(2);
    });
    let decoded = Registry::default().decode(0, &bytes);
    let mut out = serde_json::json!({
        "bytes": bytes.len(),
        "decoder": decoded.decoder,
        "template": decoded.template,
        "generation": decoded.generation,
        "fields": decoded.fields,
    });
    if let Some(a) = argent::recognize(&bytes) {
        out["argent"] = serde_json::json!({
            "generation": format!("{:?}", a.generation),
            "entrypoints": a.entrypoint_count(),
            "template_hash": hex::encode(a.template_hash),
        });
    }
    println!("{}", serde_json::to_string_pretty(&out).expect("json"));
}
