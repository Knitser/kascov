FROM rust:1.96-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release -p kascov

# ---- silverc: the SilverScript compiler, built standalone. It pins a
# different rusty-kaspa rev than kascov, so it can't be a workspace dep — we
# clone the language repo, drop in our tiny CLI (tools/silverc/main.rs), and
# build a `silverc` binary the worker shells out to for /compile. ----
FROM rust:1.96-bookworm AS silverc-build
RUN apt-get update && apt-get install -y --no-install-recommends git && rm -rf /var/lib/apt/lists/*
WORKDIR /s
RUN git clone --depth 1 https://github.com/kaspanet/silverscript.git .
COPY tools/silverc/main.rs /s/silverc/src/main.rs
RUN printf '[package]\nname = "silverc"\nversion = "0.1.0"\nedition = "2024"\n\n[dependencies]\nsilverscript-lang = { path = "../silverscript-lang" }\ndebugger-session = { path = "../debugger/session" }\n' > silverc/Cargo.toml \
    && sed -i 's/^members = \[/members = [\n    "silverc",/' Cargo.toml \
    && cargo build --release -p silverc

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl jq && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/kascov /usr/local/bin/kascov
COPY --from=silverc-build /s/target/release/silverc /usr/local/bin/silverc
COPY scripts/worker-entrypoint.sh /usr/local/bin/worker-entrypoint.sh
RUN chmod +x /usr/local/bin/worker-entrypoint.sh
ENV DB_DIR=/data
ENV SILVERC_BIN=/usr/local/bin/silverc
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/worker-entrypoint.sh"]
