# ---- kascov worker. Layered for build caching: cargo-chef compiles the
# dependency graph (the heavy rusty-kaspa git deps) into its own layer that
# only invalidates when Cargo.toml/Cargo.lock change — a source edit rebuilds
# just the workspace crates instead of the world. Cache persists across Cloud
# Builds via Kaniko's registry cache (see cloudbuild.yaml). ----
FROM rust:1.96-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /src

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release -p kascov

# ---- silverc: the SilverScript compiler, built standalone. It pins a
# different rusty-kaspa rev than kascov, so it can't be a workspace dep — we
# clone the language repo at a PINNED rev (bump SILVERSCRIPT_REV to upgrade;
# a moving clone would make every layer below uncacheable), build the heavy
# language deps against a stub main, then drop in our tiny CLI so source
# tweaks only rebuild the thin silverc crate. ----
FROM rust:1.96-bookworm AS silverc-build
RUN apt-get update && apt-get install -y --no-install-recommends git && rm -rf /var/lib/apt/lists/*
ARG SILVERSCRIPT_REV=77ebf01a381af79e9708705cf55a2186d5ea7275
WORKDIR /s
RUN git clone https://github.com/kaspanet/silverscript.git . \
    && git checkout ${SILVERSCRIPT_REV} \
    && rm -rf .git
# deps layer: same manifest as the real build, stub main
RUN mkdir -p silverc/src \
    && printf 'fn main() {}\n' > silverc/src/main.rs \
    && printf '[package]\nname = "silverc"\nversion = "0.1.0"\nedition = "2024"\n\n[dependencies]\nsilverscript-lang = { path = "../silverscript-lang" }\ndebugger-session = { path = "../debugger/session" }\n' > silverc/Cargo.toml \
    && sed -i 's/^members = \[/members = [\n    "silverc",/' Cargo.toml \
    && cargo build --release -p silverc
# the real CLI — only this crate recompiles when main.rs changes
COPY tools/silverc/main.rs /s/silverc/src/main.rs
RUN touch /s/silverc/src/main.rs && cargo build --release -p silverc

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
