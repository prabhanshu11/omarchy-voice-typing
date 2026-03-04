# ── Stage 1: Build Rust gateway ──
FROM rust:1-bookworm AS rust-builder
WORKDIR /app/gateway-rs
COPY gateway-rs/Cargo.toml gateway-rs/Cargo.lock ./
# Cache dependencies by building a dummy main — real source replaces it next
RUN mkdir src && echo 'fn main(){}' > src/main.rs && echo '' > src/lib.rs \
    && cargo build --release 2>/dev/null || true \
    && rm -rf src target/release/voice-type target/release/deps/voice_type* \
       target/release/.fingerprint/voice-type-* target/release/.fingerprint/voice_type-*
COPY gateway-rs/src ./src
RUN cargo build --release

# ── Stage 2: Build frontend SPA ──
FROM node:20-slim AS web-builder
WORKDIR /app/web
COPY web/package*.json ./
RUN npm ci
COPY web/ .
ENV NODE_ENV=production
ENV VITE_BASE_PATH=/voice-typing/
RUN npx vite build

# ── Stage 3: Runtime ──
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=rust-builder /app/gateway-rs/target/release/voice-type /usr/local/bin/
COPY --from=web-builder /app/web/dist /app/web/dist
COPY config /app/config

# Demo data for profiling dashboard (session logs, latency, transcripts)
COPY deploy/demo-data/logs/ /app/logs/
COPY deploy/demo-data/transcripts/ /app/transcripts/
RUN mkdir -p /app/recordings

WORKDIR /app
ENV PORT=8766
ENV RUST_LOG=info
EXPOSE 8766
CMD ["voice-type"]
