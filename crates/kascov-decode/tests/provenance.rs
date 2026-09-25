//! Every program fixture says where it came from, and says it about the file
//! that is actually there: `fixtures/provenance.json` must list each `.bin` and
//! `.hex` under `fixtures/` and `tests/fixtures/` once, with the file's own size
//! and BLAKE2b-256, and exactly one origin: a chain reveal, a whole witness, or
//! a written note. A fixture added without saying where it came from fails here.

use std::collections::BTreeSet;
use std::path::Path;

fn blake2b_256(bytes: &[u8]) -> String {
    hex::encode(
        blake2b_simd::Params::new()
            .hash_length(32)
            .hash(bytes)
            .as_bytes(),
    )
}

fn fixture_files(root: &Path) -> BTreeSet<String> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).expect("fixture dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                walk(&path, root, out);
            } else if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("bin" | "hex")
            ) {
                let rel = path.strip_prefix(root).expect("under the crate");
                out.insert(
                    rel.components()
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/"),
                );
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(&root.join("fixtures"), root, &mut out);
    walk(&root.join("tests/fixtures"), root, &mut out);
    out
}

fn txid(v: &serde_json::Value, what: &str) {
    let s = v.as_str().unwrap_or_else(|| panic!("{what}: not a string"));
    assert!(
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()),
        "{what}: {s:?} is not a transaction id"
    );
}

#[test]
fn every_fixture_says_where_it_came_from() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let doc: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/provenance.json")).unwrap();
    let entries = doc["fixtures"].as_array().expect("fixtures array");

    let listed: BTreeSet<String> = entries
        .iter()
        .map(|e| e["path"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(listed.len(), entries.len(), "a fixture is listed twice");
    let on_disk = fixture_files(root);
    let missing: Vec<_> = on_disk.difference(&listed).collect();
    let stale: Vec<_> = listed.difference(&on_disk).collect();
    assert!(
        missing.is_empty(),
        "fixtures with no provenance entry: {missing:?}"
    );
    assert!(
        stale.is_empty(),
        "provenance entries for files that are gone: {stale:?}"
    );

    let (mut chain, mut witness, mut noted) = (0, 0, 0);
    for e in entries {
        let path = e["path"].as_str().unwrap();
        let raw = std::fs::read(root.join(path)).unwrap();
        let bytes = if path.ends_with(".hex") {
            // hex fixtures wrap their lines; the bytes are the digits alone
            let digits: String = String::from_utf8(raw).unwrap().split_whitespace().collect();
            hex::decode(digits).unwrap_or_else(|e| panic!("{path}: {e}"))
        } else {
            raw
        };
        assert_eq!(
            e["bytes"].as_u64().unwrap() as usize,
            bytes.len(),
            "{path}: size"
        );
        assert_eq!(
            e["blake2b_256"].as_str().unwrap(),
            blake2b_256(&bytes),
            "{path}: hash"
        );

        let kinds = ["chain", "witness", "origin"]
            .iter()
            .filter(|k| !e[**k].is_null())
            .count();
        assert_eq!(kinds, 1, "{path}: exactly one of chain, witness, origin");

        if let Some(nets) = e["chain"].as_object() {
            chain += 1;
            assert!(!nets.is_empty(), "{path}: a chain entry names a network");
            for (net, c) in nets {
                assert!(
                    net == "mainnet" || net == "testnet-10",
                    "{path}: network {net}"
                );
                txid(&c["outpoint"]["txid"], path);
                txid(&c["revealed_by"]["txid"], path);
                txid(&c["covenant_id"], path);
                assert!(
                    c["reveals_matching"].as_u64().unwrap() >= 1,
                    "{path}: a verified reveal"
                );
                assert!(c["reveals_matching"].as_u64() <= c["reveals_checked"].as_u64());
            }
        } else if let Some(nets) = e["witness"].as_object() {
            witness += 1;
            for w in nets.values() {
                txid(&w["spends"]["txid"], path);
                txid(&w["in_transaction"]["txid"], path);
            }
        } else {
            noted += 1;
            assert!(
                e["origin"].as_str().is_some_and(|s| s.len() > 20),
                "{path}: an origin note says something"
            );
        }
    }
    // the file's own summary, so a regenerated manifest that quietly lost its
    // chain evidence shows up as a changed count rather than passing
    assert!(
        chain >= 250,
        "only {chain} fixtures carry a verified chain reveal"
    );
    assert_eq!(chain + witness + noted, entries.len());
}
