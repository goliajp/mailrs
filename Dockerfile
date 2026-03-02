# stage 1: build rust binary
FROM rust:1-bookworm AS rust-builder

WORKDIR /build

# copy workspace manifests first for layer caching
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/

RUN cargo build --release --bin mailrs-server

# stage 2: build frontend
FROM node:22-bookworm-slim AS web-builder

WORKDIR /build

RUN npm install -g bun

COPY web/package.json web/bun.lock* ./
RUN bun install --frozen-lockfile

COPY web/ ./
RUN bun run build

# stage 3: runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=rust-builder /build/target/release/mailrs-server /usr/local/bin/mailrs-server
COPY --from=web-builder /build/dist /opt/mailrs/web

# default env vars
ENV MAILRS_HOSTNAME=mx.mailrs.local \
    MAILRS_MAILDIR=/data/maildir \
    MAILRS_PORT=25 \
    MAILRS_SUBMISSION_PORT=587 \
    MAILRS_SMTPS_PORT=465 \
    MAILRS_IMAP_PORT=143 \
    MAILRS_WEB_PORT=3100 \
    MAILRS_PG_URL=postgres://mailrs:mailrs@postgres:5432/mailrs \
    MAILRS_VALKEY_URL=redis://valkey:6379 \
    MAILRS_WEB_STATIC_DIR=/opt/mailrs/web \
    MAILRS_ACME_DIR=/data/acme

EXPOSE 25 80 587 465 143 3100

VOLUME ["/data", "/certs"]

ENTRYPOINT ["mailrs-server"]
