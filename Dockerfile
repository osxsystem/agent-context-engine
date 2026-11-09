# syntax=docker/dockerfile:1.7

FROM rust:1-bookworm AS build
# librocksdb-sys (SurrealDB kv-rocksdb) runs bindgen at build time and needs libclang.
RUN apt-get update \
 && apt-get install -y --no-install-recommends clang libclang-dev \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
# COPY keeps host mtimes, which can be older than artifacts already in the cached
# target dir; touching the crate roots forces cargo to recompile this crate.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    touch src/main.rs src/lib.rs \
 && cargo build --release --locked --bin context-engine-rs \
 && cp target/release/context-engine-rs /usr/local/bin/context-engine-rs

# Optional: `docker build --target test --build-arg TEST_FILTER=... .`
FROM build AS test
ARG TEST_FILTER=""
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo test --release --locked --lib -- ${TEST_FILTER}

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tini \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/bin/context-engine-rs /usr/local/bin/context-engine-rs
ENV HOME=/home/engine \
    CONTEXT_ENGINE_BIND=0.0.0.0 \
    CONTEXT_ENGINE_PORT=6699
RUN mkdir -p /home/engine/.vibervn/context-engine /workspace
VOLUME ["/home/engine/.vibervn"]
EXPOSE 6699
# tini reaps the per-repo worker processes the router spawns and forwards SIGTERM.
ENTRYPOINT ["/usr/bin/tini", "--", "context-engine-rs"]
