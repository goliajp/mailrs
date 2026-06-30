# stage 1a: cargo-chef base image — cache layer for chef tool itself
FROM rust:1-trixie AS chef
RUN cargo install cargo-chef --locked --version ^0.1
WORKDIR /build

# stage 1b: planner — produces recipe.json describing the dep graph.
# This layer is cheap (no compile), but its output (recipe.json) is the
# cache key for the heavy cook stage below.
FROM chef AS planner
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/
RUN cargo chef prepare --recipe-path recipe.json

# stage 1c: builder — first cooks all external deps (cached), then
# builds the workspace + mailrs-server (re-runs on any crate code change).
#
# Version is injected at build time from the git tag (e.g. v1.7.134 → 1.7.134).
# Repo's Cargo.toml stays at 0.0.0 (placeholder) so there's no bump commit on
# release; the real version lives only in the tag and the built artifact.
FROM chef AS rust-builder
ARG VERSION=0.0.0

# Cook external deps — buildx GHA cache makes this layer skip entirely
# on subsequent builds where Cargo.toml/Cargo.lock are unchanged.
COPY --from=planner /build/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/target \
    cargo chef cook --release --recipe-path recipe.json

# Now copy the real source. Layer below invalidates on any crate code
# change, but external deps stay cached above.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/

# Patch the workspace root version BEFORE cargo build so the resolver
# picks it up. Only the workspace.package.version line; member crates
# inherit via `version.workspace = true`.
RUN sed -i "0,/^version = \".*\"/s//version = \"$VERSION\"/" Cargo.toml

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/target \
    cargo build --release --bin mailrs-server --features spg,render-preview,core-rpc \
    && cargo build --release --bin mailrs-receiver \
    && cargo build --release --bin mailrs-webapi \
    && cargo build --release --bin mailrs-sender \
    && cp /build/target/release/mailrs-server /usr/local/bin/mailrs-server \
    && cp /build/target/release/mailrs-receiver /usr/local/bin/mailrs-receiver \
    && cp /build/target/release/mailrs-webapi /usr/local/bin/mailrs-webapi \
    && cp /build/target/release/mailrs-sender /usr/local/bin/mailrs-sender

# stage 2: build frontend
FROM oven/bun:1-debian AS web-builder

ARG VERSION=0.0.0

WORKDIR /build

COPY web/package.json web/bun.lock ./
RUN sed -i "s/\"version\": \"[^\"]*\"/\"version\": \"$VERSION\"/" package.json
RUN bun install --frozen-lockfile

COPY web/ ./
RUN bun run build

# stage 3: runtime
FROM debian:trixie-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl libcap2-bin \
    && rm -rf /var/lib/apt/lists/*

# run as non-root user with EXPLICIT UID/GID 10001 so prod can chown
# host-bind-mounted secrets (certs, ACME) to a stable id. Without the
# explicit `-u/-g`, `useradd -r` picks the next free system uid (typically
# 999 on a stock debian:trixie-slim), which can shift between base-image
# versions and silently break bind-mount permissions. See
# .claude/memory/ghcr-cert-perm-trap.md for the v1.7.89 incident.
RUN groupadd -r -g 10001 mailrs && useradd -r -u 10001 -g mailrs -d /data -s /sbin/nologin mailrs

COPY --from=rust-builder /usr/local/bin/mailrs-server /usr/local/bin/mailrs-server
# receiver split (P6): the standalone receiver binary ships in the same
# image. Idle unless a container's entrypoint is overridden to
# mailrs-receiver (the receiver-split topology); the default mailrs-server
# entrypoint never touches it. One image, two roles.
COPY --from=rust-builder /usr/local/bin/mailrs-receiver /usr/local/bin/mailrs-receiver
# Phase 3 (webapi split): same one-image-many-roles pattern. Idle unless
# the container's entrypoint is overridden to `mailrs-webapi`. Talks to
# the core via mailrs-core-api over HTTP — no PG access in this binary.
COPY --from=rust-builder /usr/local/bin/mailrs-webapi /usr/local/bin/mailrs-webapi
# Phase 4 (sender split): outbound delivery / webhook / DMARC report
# worker. Idle until entrypoint is overridden to `mailrs-sender`. Talks
# to core via mailrs-core-api.
COPY --from=rust-builder /usr/local/bin/mailrs-sender /usr/local/bin/mailrs-sender
COPY --from=web-builder /build/dist /opt/mailrs/web

# Grant the binary capability to bind privileged ports (< 1024) so it
# can listen on 25 / 110 / 143 / 465 / 587 / 993 / 995 while running as
# the non-root mailrs user. Without this, mailrs's bind fallback shifts
# every privileged listener to <port+1000> (e.g. SMTP 25 → 1025) and
# external mail traffic stops landing. Discovered in v1.7.91 cutover.
RUN setcap 'cap_net_bind_service=+ep' /usr/local/bin/mailrs-server
# the receiver binary binds the same privileged SMTP ports (25 / 465 / 587)
# when run as the receiver-split front door, so it needs the same capability.
RUN setcap 'cap_net_bind_service=+ep' /usr/local/bin/mailrs-receiver

# create data directories with correct ownership
RUN mkdir -p /data/maildir /data/acme /certs && chown -R mailrs:mailrs /data

# default env vars
# NB: MAILRS_KEVY_URL is intentionally NOT set. Phase C (v1.7.95+) runs
# kevy in-process (MAILRS_KEVY_DATA_DIR); there is no network kevy
# container. Set MAILRS_KEVY_URL only in the receiver-split topology (P6)
# when a real shared kevy-server is deployed — and use a kevy://host:port
# URL, not redis://. Leaving it unset keeps the anti subsystems
# (greylist / rate / auth-guard) on their correct in-process backends.
ENV MAILRS_HOSTNAME=mx.mailrs.local \
    MAILRS_MAILDIR=/data/maildir \
    MAILRS_PORT=25 \
    MAILRS_SUBMISSION_PORT=587 \
    MAILRS_SMTPS_PORT=465 \
    MAILRS_IMAP_PORT=143 \
    MAILRS_IMAPS_PORT=993 \
    MAILRS_POP3_PORT=110 \
    MAILRS_WEB_PORT=3100 \
    MAILRS_PG_URL=postgres://mailrs:mailrs@postgres:5432/mailrs \
    MAILRS_WEB_STATIC_DIR=/opt/mailrs/web \
    MAILRS_ACME_DIR=/data/acme

EXPOSE 25 80 110 587 465 143 993 995 3100 4190

VOLUME ["/data", "/certs"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -sf http://localhost:3100/api/health || exit 1

USER mailrs

ENTRYPOINT ["mailrs-server"]
