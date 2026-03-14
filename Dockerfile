# stage 1: build rust binary
FROM rust:1-bookworm AS rust-builder

WORKDIR /build

# copy manifests first for dependency layer caching
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/

# mount cargo registry + git cache + target dir for incremental builds
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/target \
    cargo build --release --bin mailrs-server \
    && cp /build/target/release/mailrs-server /usr/local/bin/mailrs-server

# stage 2: build frontend
FROM oven/bun:1-debian AS web-builder

WORKDIR /build

COPY web/package.json web/bun.lock ./
RUN bun install --frozen-lockfile

COPY web/ ./
RUN bun run build

# stage 3: runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

# run as non-root user
RUN groupadd -r mailrs && useradd -r -g mailrs -d /data -s /sbin/nologin mailrs

COPY --from=rust-builder /usr/local/bin/mailrs-server /usr/local/bin/mailrs-server
COPY --from=web-builder /build/dist /opt/mailrs/web

# create data directories with correct ownership
RUN mkdir -p /data/maildir /data/acme /certs && chown -R mailrs:mailrs /data

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

EXPOSE 25 80 110 587 465 143 993 995 3100 4190

VOLUME ["/data", "/certs"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -sf http://localhost:3100/api/health || exit 1

USER mailrs

ENTRYPOINT ["mailrs-server"]
