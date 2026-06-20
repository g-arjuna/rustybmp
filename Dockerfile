# ─── Stage 1: Build ──────────────────────────────────────────────────────────
FROM rust:1.85-slim-bookworm AS builder

WORKDIR /build

# Layer: system deps for DuckDB bundled build + libssl for rdkafka
RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake pkg-config libssl-dev ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# Layer: cache dependencies (only Cargo manifests)
COPY Cargo.toml Cargo.lock ./
COPY crates/rbmp-core/Cargo.toml       crates/rbmp-core/Cargo.toml
COPY crates/rbmp-rib/Cargo.toml        crates/rbmp-rib/Cargo.toml
COPY crates/rbmp-store/Cargo.toml      crates/rbmp-store/Cargo.toml
COPY crates/rbmp-enrichment/Cargo.toml crates/rbmp-enrichment/Cargo.toml
COPY crates/rbmp-kafka/Cargo.toml      crates/rbmp-kafka/Cargo.toml
COPY crates/rbmp-mrt/Cargo.toml        crates/rbmp-mrt/Cargo.toml
COPY crates/rbmp-nats/Cargo.toml       crates/rbmp-nats/Cargo.toml
COPY crates/rbmp-server/Cargo.toml     crates/rbmp-server/Cargo.toml

# Stub src/ so cargo can resolve the workspace
RUN mkdir -p crates/rbmp-core/src && echo "pub fn stub() {}" > crates/rbmp-core/src/lib.rs \
 && mkdir -p crates/rbmp-rib/src      && echo "pub fn stub() {}" > crates/rbmp-rib/src/lib.rs \
 && mkdir -p crates/rbmp-store/src    && echo "pub fn stub() {}" > crates/rbmp-store/src/lib.rs \
 && mkdir -p crates/rbmp-enrichment/src && echo "pub fn stub() {}" > crates/rbmp-enrichment/src/lib.rs \
 && mkdir -p crates/rbmp-kafka/src    && echo "pub fn stub() {}" > crates/rbmp-kafka/src/lib.rs \
 && mkdir -p crates/rbmp-mrt/src      && echo "pub fn stub() {}" > crates/rbmp-mrt/src/lib.rs \
 && mkdir -p crates/rbmp-nats/src     && echo "pub fn stub() {}" > crates/rbmp-nats/src/lib.rs \
 && mkdir -p crates/rbmp-server/src   && echo "fn main() {}" > crates/rbmp-server/src/main.rs
RUN cargo build --release --workspace 2>/dev/null || true

# Now copy full source and rebuild (only changed crates rebuild)
COPY crates/ crates/
RUN cargo build --release --bin rustybmp --bin rbmp-collector

# ─── Stage 2: Runtime ────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/target/release/rustybmp       /usr/local/bin/rustybmp
COPY --from=builder /build/target/release/rbmp-collector /usr/local/bin/rbmp-collector
COPY config/rustybmp.toml.example                        /app/rustybmp.toml.example

RUN mkdir -p /data && useradd -m -u 1001 rustybmp && chown -R rustybmp /app /data
USER rustybmp

# BMP receiver | HTTP API | Collector protocol
EXPOSE 5000 7878 5001

VOLUME ["/data"]

ENV RUST_LOG=info
CMD ["rustybmp", "/app/rustybmp.toml"]
