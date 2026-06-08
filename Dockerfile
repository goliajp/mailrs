# stage 1: build rust binary
FROM rust:1-trixie AS rust-builder

# Version is injected at build time from the git tag (e.g. v1.7.134 → 1.7.134).
# Repo's Cargo.toml stays at 0.0.0 (placeholder) so there's no bump commit on
# release; the real version lives only in the tag and the built artifact.
ARG VERSION=0.0.0

WORKDIR /build

# copy manifests first for dependency layer caching
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/

# patch the workspace root version BEFORE cargo build so the resolver picks
# it up. Only the workspace.package.version line; member crates inherit via
# `version.workspace = true`.
RUN sed -i "0,/^version = \".*\"/s//version = \"$VERSION\"/" Cargo.toml

# mount cargo registry + git cache + target dir for incremental builds
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/target \
    cargo build --release --bin mailrs-server \
    && cp /build/target/release/mailrs-server /usr/local/bin/mailrs-server

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
COPY --from=web-builder /build/dist /opt/mailrs/web

# Grant the binary capability to bind privileged ports (< 1024) so it
# can listen on 25 / 110 / 143 / 465 / 587 / 993 / 995 while running as
# the non-root mailrs user. Without this, mailrs's bind fallback shifts
# every privileged listener to <port+1000> (e.g. SMTP 25 → 1025) and
# external mail traffic stops landing. Discovered in v1.7.91 cutover.
RUN setcap 'cap_net_bind_service=+ep' /usr/local/bin/mailrs-server

# create data directories with correct ownership
RUN mkdir -p /data/maildir /data/acme /certs && chown -R mailrs:mailrs /data

# default env vars
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
    MAILRS_KEVY_URL=redis://kevy:6379 \
    MAILRS_WEB_STATIC_DIR=/opt/mailrs/web \
    MAILRS_ACME_DIR=/data/acme

EXPOSE 25 80 110 587 465 143 993 995 3100 4190

VOLUME ["/data", "/certs"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -sf http://localhost:3100/api/health || exit 1

USER mailrs

ENTRYPOINT ["mailrs-server"]
