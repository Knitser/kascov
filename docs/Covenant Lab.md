# Covenant Lab

`kascov-lab` — we don't just observe covenants, we make them. The lab creates a real covenant lifecycle on **testnet-10** that the explorer then indexes and traces: the end-to-end proof. (Proven July 2: covenant `3af0fffe…` — genesis → 2 transitions → burn, fully traced.)

## Start here — one command

Deploy a real contract *and* run it on-chain in a single command; open the kascov link it prints (turn on *nerd mode*) to watch the coin reveal itself as its named contract:

```sh
cargo run -p kascov-lab -- escrow-demo     # an escrow deploys, then the arbiter settles it to the buyer
cargo run -p kascov-lab -- contract-demo   # a Mecenas deploys, then reclaims itself → revealed on kascov
cargo run -p kascov-lab -- examples        # print every copy-paste recipe
```

First run only: `cargo run -p kascov-lab -- keygen`, fund the printed address at <https://faucet-testnet.kaspanet.io>, then any demo works. Every command has `--help`.

### Commands at a glance

| command | what it does |
|---|---|
| `keygen` | make/print a throwaway testnet key + address |
| `balance` | show the address balance |
| `examples` | print all the recipes (no key/network needed) |
| `contract-demo` | deploy a Mecenas + reclaim it → revealed on kascov (one command) |
| `escrow-demo` | deploy an escrow + settle it to the buyer (one command) |
| `demo` | raw covenant lifecycle: genesis → N transitions → burn |
| `deploy` | birth a compiled contract as a real covenant (hidden p2sh state) |
| `spend` | satisfy an entrypoint → reveal the program on-chain (`reclaim` \| `cold` \| `inherit` \| `receive` \| `refresh`) |
| `settle-escrow` | satisfy Escrow.spend as arbiter, forcing the payout to a party |

## Why no SilverScript needed

A covenant is a *consensus* construct (KIP-20), not a language construct. Any v1 transaction output carrying a `CovenantBinding` forms one — the script can be a plain pay-to-pubkey. The lab:

1. **Genesis** — spends a funding UTXO; output 0 carries `CovenantBinding { authorizing_input: 0, covenant_id }` where the id is computed with the consensus helper `covenant_id(outpoint, [(0, &output)])` (BLAKE2b-256, domain `"CovenantID"` — see [[Toccata Protocol Notes]]). The [[Sync Engine#Classification]] now validates exactly this construction when classifying genesis events — the lab and the indexer literally call the same function.
2. **Transitions** — each spends the previous covenant UTXO and re-binds the same id (continuation). The spent UTXO's `covenant_id` is set in the signing entry.
3. **Burn** — spends the covenant UTXO into a plain output. Lineage ends.

Transactions are `TX_VERSION_TOCCATA` (v1), schnorr-signed with rusty-kaspa's own `sign()` helper (which also sets the v1 input compute-budget commitment). Storage mass is left at 0 — the node computes and sets it during mempool validation.

## Running it

```sh
kascov-lab keygen                      # prints a kaspatest: address
# fund it: https://faucet-testnet.kaspanet.io (Cloudflare-gated — use a browser)
kascov-lab balance
kascov-lab demo --transitions 2        # genesis → 2 transitions → burn

# watch kascov catch it (start sync BEFORE the demo to capture all events):
kascov --network testnet-10 sync --follow &
kascov --network testnet-10 trace <covenant-id-from-demo>

# or just paste the demo's txid into the search box at kascov.io —
# it lands on the coin with the event highlighted, live within seconds
```

Key file: `/tmp/kascov-lab-key.hex` (throwaway testnet key). TN10 min relay fee ≈ 166k sompi for a 1-in-1-out tx; the lab uses 500k.

## Design note

The lab deliberately breaks [[Architecture]] Rule 1 (it imports kaspa crates directly) — building and signing transactions *is* the upstream surface, and wrapping it in our model types would only re-state the kaspa API with extra steps.


## Deploying a compiled contract

The generator on [kascov.io/decode](https://kascov.io/decode) turns any recognized SilverScript contract into *your* instance: edit the constructor args, copy the compiled hex, then:

```sh
cargo run -p kascov-lab -- keygen          # prints address, pubkey, blake2b(pubkey)
# fund the address at https://faucet-testnet.kaspanet.io, then:
cargo run -p kascov-lab -- deploy --program-hex <hex> --value 1000000000
```

The coin is born with a **P2SH commitment** state (`OpBlake2b <blake2b-256(program)> OpEqual`) bound to a fresh covenant id, and appears on the explorer within ~a minute — as a `p2sh commitment` (the program is hidden behind the hash).

### No toolchain at all: one-click web deploy

The builder on [kascov.io/decode](https://kascov.io/decode) can skip the CLI entirely: fill in the parties and amounts, click deploy, and the site POSTs the compiled hex to the worker's `/data/testnet-10/deploy` endpoint. The **server** births the covenant with its own custodial faucet key. Deliberately narrow: the route only exists when the worker is armed with `KASCOV_DEPLOY_KEY` **and** the network is testnet-10 (404 otherwise — never mainnet), value is capped at 1–10 TKAS, and it's rate-limited globally (~144/day) and per IP (20/day). The tx-building code is the same [[Architecture]] `kascov-labkit` library the CLI uses.

### Proven in production (July 10, 2026)

The whole loop ran end-to-end on the live site: `POST /deploy` birthed Mecenas covenant `b4ade48e3ad1…` on TN10 → the indexer picked it up with **provable genesis** → `kascov-lab spend --entrypoint reclaim` reclaimed it → the coin revealed itself as *SilverScript · Mecenas* on [kascov.io](https://kascov.io), permanently.

## Revealing it — spend the contract on-chain

Spending the coin reveals the program, so kascov shows it as your named contract *for everyone, permanently* (`revealed at spend — SilverScript · Mecenas`, with your args labeled). The lab satisfies the **pure-signature** entrypoints (Mecenas `reclaim`, LastWill `cold`/`inherit`) — they need only a signature from the matching key:

```sh
# the coin's funder/cold/inheritor hash must be YOUR key's blake2b (keygen prints it)
cargo run -p kascov-lab -- spend --program-hex <same hex> --entrypoint reclaim
```

The whole loop — emit a reclaimable Mecenas, deploy it, reclaim it — in one command:

```sh
cargo run -p kascov-lab -- contract-demo
```

**How the spend works:** the unlocking script is `push(pubkey) ++ push(sig) ++ [push(selector)] ++ push(program)` — the revealed contract program as the final push. The signature is the standard Schnorr sighash (`SIG_HASH_ALL`) computed over the P2SH UTXO; the entrypoint selector is a small-int push (Mecenas `reclaim`=1). The per-input compute budget is committed via `ComputeBudget` (1 unit = 10 000 script units; a signature spend needs ~1, and the fee scales as 100 sompi × compute mass).

## Settling an escrow — an output-constrained spend

`Escrow.spend` also checks the transaction's *outputs* (`tx.outputs[0]` must pay the buyer or seller exactly `value − 1000`), so the lab both satisfies the arbiter signature **and** builds those outputs, funding the real network fee from a second plain input:

```sh
# arbiter (your key) releases the escrowed funds to a party
cargo run -p kascov-lab -- settle-escrow --program-hex <hex> --release-to buyer
```

The whole loop — deploy an escrow, then settle it — in one command:

```sh
cargo run -p kascov-lab -- escrow-demo
```

Proven on TN10: covenant `da2fe117…` deployed, then settled 4.99999 TKAS to the buyer — now shows `revealed at spend — SilverScript · Escrow` with arbiter/buyer/seller labeled.

## The other output-constrained entrypoints — wired

Mecenas `receive` and LastWill `refresh` use the same technique as `settle-escrow` and dispatch through labkit's `spend_constrained`:

```sh
# Mecenas.receive — selector 0, NO signature (anyone may trigger a payout period):
#   outputs[0] pays the recipient the pledge, outputs[1] re-commits the remainder
cargo run -p kascov-lab -- spend --program-hex <hex> --entrypoint receive

# LastWill.refresh — selector 2, signed with the HOT key:
#   outputs[0] re-commits the same P2SH state (resets the inheritance clock)
cargo run -p kascov-lab -- spend --program-hex <hex> --entrypoint refresh
```

Every SilverScript template entrypoint the lab knows is now spendable: `reclaim` · `cold` · `inherit` (pure-signature), `receive` · `refresh` (output-constrained), plus `settle-escrow` for Escrow.
