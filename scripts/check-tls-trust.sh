#!/usr/bin/env bash
# check-tls-trust.sh — nothing verifies a peer against a browser's roots
# alone.
#
# 2026-08-17: mail to every Microsoft-hosted domain was undeliverable.
# Their chain anchors at `DigiCert Global Root CA`, valid to 2031 and
# present in the container's `ca-certificates`, and `webpki-roots` 1.0.8
# does not ship it — it carries DigiCert G2, G3, G4, Assured ID G2/G3
# and both G5 roots, and not the original. `webpki-roots` tracks
# Mozilla's **browser** program; an SMTP peer is not a browser, and
# neither is an identity provider or an unsubscribe endpoint.
#
# The bug was fixed in one place and existed in three. This is the
# other two plus every future one:
#
#   * a `RootCertStore` built from `TLS_SERVER_ROOTS` without the
#     platform store beside it (`smtp-client/src/dane.rs` had one, so
#     DANE usages 0 and 1 — which require standard PKIX *as well as* a
#     TLSA match — were checked against the short store)
#   * reqwest's webpki-roots-only TLS features, which is what
#     `rustls-tls` means in 0.12 (`webapi` and `mail-builder` had them;
#     `webapi` talks to identity providers and to arbitrary
#     List-Unsubscribe URLs). In 0.13 `rustls` pulls
#     `rustls-platform-verifier` and is fine.
#
# A crate that genuinely wants pinned or DANE-only trust builds its own
# verifier and passes it to `try_starttls_with_config`; it does not
# reach for the default store, so it does not trip this.
set -euo pipefail
export LC_ALL=C
cd "$(dirname "$0")/.."

fail=0

# --- the trust store, in code -------------------------------------
#
# `trust.rs` is the one place allowed to name it: it merges the two
# sets, and its own test asserts the platform store never replaces the
# compiled-in one.
while IFS= read -r hit; do
    file="${hit%%:*}"
    case "$file" in
        crates/smtp-client/src/trust.rs) continue ;;
    esac
    echo "!! $hit"
    echo "   builds trust from webpki-roots alone; use mailrs_smtp_client::pkix_root_store()"
    fail=1
done < <(grep -rn "TLS_SERVER_ROOTS" --include='*.rs' crates/ || true)

# --- the trust store, in a manifest -------------------------------
#
# Named literally rather than by version, because the defect is the
# feature and a future reqwest could spell it the same way.
while IFS= read -r hit; do
    echo "!! $hit"
    echo "   selects webpki-roots-only TLS; on reqwest 0.13 use features = [\"rustls\"]"
    fail=1
done < <(grep -rn 'rustls-tls-webpki-roots\|"rustls-tls"' --include='Cargo.toml' Cargo.toml crates/ || true)

if [ "$fail" -ne 0 ]; then
    echo
    echo "See .claude/incidents/INC-2026-08-17-microsoft-mail-untrusted-root.md"
    exit 1
fi

echo "TLS trust OK — every peer is verified against the platform store too"
