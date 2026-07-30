//! Compute a KIP-20 covenant id from a claimed genesis outpoint and the
//! authorized outputs — settles genesis-vs-continuation disputes with the
//! node's own consensus function.
//!
//!   cargo run -p kascov-core --example covid -- <txid>:<index> <out_index>:<value>:<script_hex> [...]

use kascov_core::node::compute_covenant_id;
use kascov_core::{Outpoint, TxId};
use std::str::FromStr;

fn main() {
    let mut args = std::env::args().skip(1);
    let outpoint_arg = args.next().expect("usage: covid <txid>:<index> <idx>:<value>:<script_hex>…");
    let (txid, idx) = outpoint_arg.split_once(':').expect("outpoint as txid:index");
    let outpoint = Outpoint {
        txid: TxId::from_str(txid).expect("valid txid"),
        index: idx.parse().expect("outpoint index"),
    };
    let outs: Vec<(u32, u64, u16, Vec<u8>)> = args
        .map(|a| {
            let mut p = a.splitn(3, ':');
            let index: u32 = p.next().unwrap().parse().expect("output index");
            let value: u64 = p.next().unwrap().parse().expect("output value");
            let script = hex::decode(p.next().expect("script hex")).expect("hex script");
            (index, value, 0u16, script)
        })
        .collect();
    let borrowed: Vec<(u32, u64, u16, &[u8])> =
        outs.iter().map(|(i, v, ver, s)| (*i, *v, *ver, s.as_slice())).collect();
    println!("{}", compute_covenant_id(&outpoint, &borrowed));
}
