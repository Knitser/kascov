# silverc — the SilverScript compiler CLI

The worker's `/compile` endpoint (verify-and-publish + the no-code builder)
shells out to a `silverc` binary that compiles SilverScript source + constructor
args to Kaspa script hex. SilverScript lives at
[kaspanet/silverscript](https://github.com/kaspanet/silverscript) and pins a
different rusty-kaspa version than kascov, so it is built **standalone** (not a
workspace dependency) and invoked as an external binary.

## Build it
```bash
git clone --depth 1 https://github.com/kaspanet/silverscript.git
cd silverscript
mkdir -p silverc/src && cp <this repo>/tools/silverc/main.rs silverc/src/main.rs
cat > silverc/Cargo.toml <<'TOML'
[package]
name = "silverc"
version = "0.1.0"
edition = "2024"
[dependencies]
silverscript-lang = { path = "../silverscript-lang" }
debugger-session = { path = "../debugger/session" }
TOML
# add "silverc" to the workspace `members` in Cargo.toml, then:
cargo build -p silverc --release   # → target/release/silverc
```

## Wire it
Run the worker with `SILVERC_BIN=/abs/path/to/silverc`. The `/compile` handler
pipes the source to `silverc - <arg> <arg> …` and returns the hex.

Usage: `silverc <file.sil|-> [ctor_arg …]`  ( `-` reads source from stdin ).

## Deploy (prod)
The Cloud Run worker image must bundle the `silverc` binary — add a build stage
that clones + builds silverscript and `COPY`s the binary in, then set
`SILVERC_BIN` in the service env. Until that lands, `/compile` returns
"compiler isn't available" (graceful) and the playground shows that message.
