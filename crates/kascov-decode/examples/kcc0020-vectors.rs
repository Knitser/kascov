//! Print every KCC-0020 vector this crate derives from the merged draft
//! (kaspanet/kccs at ea5176aa), for publication and for cross-checking by
//! other implementations:
//!
//!   cargo run -p kascov-decode --example kcc0020-vectors

use kascov_decode::kcc0020::{self, State, Template, TransferArgs};
use kascov_decode::kcc1;

fn main() {
    println!("KCC-0020 derived vectors — spec commit {}", kcc0020::SPEC_COMMIT);
    println!();
    println!("dispatch tags (KCC-1 §6.1, BLAKE3):");
    println!("  {} = {}", kcc0020::TRANSFER_SIGNATURE, hex::encode(kcc0020::transfer_tag()));
    println!("  {} = {}", kcc0020::DELEGATOR_SIGNATURE, hex::encode(kcc0020::delegator_tag()));

    let state = State {
        amount: 1_000_000,
        owner: [0x44; 32],
        owner_scheme: kcc0020::OWNER_P2PK_SCHNORR,
        borrow_scheme: 0x00,
        borrow_guard: [0x00; 32],
        extension_commitment: [0x00; 32],
    };
    println!();
    println!("KCC20State (§2), six PushExplicit fields, {} bytes:", kcc0020::STATE_LEN);
    println!("  amount=1000000 owner=44^32 owner_scheme=00 borrow_scheme=00 guard=00^32 ext=00^32");
    println!("  encoded = {}", hex::encode(state.encode().unwrap()));

    let template = Template { prefix: vec![0x6b], suffix: vec![0x6c, 0x75, 0x51] };
    println!();
    println!("template prefix=6b suffix=6c7551:");
    println!("  TemplateHash (BLAKE3) = {}", hex::encode(template.hash()));
    println!("  program               = {}", hex::encode(template.program(&state).unwrap()));
    println!("  envelope SPK (BLAKE2b)= {}", hex::encode(template.reconstruct_spk(&state).unwrap()));

    let args = TransferArgs { next_states: vec![state], witness: vec![0x00] };
    println!();
    println!("transfer args, field-major SOA lowering (K1 §5.6), n=1, witness=00:");
    println!("  pushes = {}", hex::encode(args.encode().unwrap()));

    let preimage = [0x5a; 32];
    println!();
    println!("hash-chain borrow (§5): guard = Hash(preimage), BLAKE3:");
    println!("  preimage = {}", hex::encode(preimage));
    println!("  guard    = {}", hex::encode(kcc1::hash(&preimage)));
}
