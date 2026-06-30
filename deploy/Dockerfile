FROM debian:trixie-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY bin/mailrs-server /usr/local/bin/mailrs-server
# Phase 3 — additional binaries from the same image. Optional copies
# (the receiver-split flow + the upcoming webapi front). Both fall back
# gracefully if the binary is absent (entrypoint override required).
COPY --chmod=0755 bin/mailrs-webapi* /usr/local/bin/
COPY web /opt/mailrs/web

ENV MAILRS_HOSTNAME=mx.mailrs.local \
    MAILRS_MAILDIR=/data/maildir \
    MAILRS_PORT=25 \
    MAILRS_SUBMISSION_PORT=587 \
    MAILRS_SMTPS_PORT=465 \
    MAILRS_IMAP_PORT=143 \
    MAILRS_IMAPS_PORT=993 \
    MAILRS_WEB_PORT=3100 \
    MAILRS_WEB_STATIC_DIR=/opt/mailrs/web \
    MAILRS_ACME_DIR=/data/acme

EXPOSE 25 80 587 465 143 993 3100

VOLUME ["/data", "/certs"]

ENTRYPOINT ["mailrs-server"]
