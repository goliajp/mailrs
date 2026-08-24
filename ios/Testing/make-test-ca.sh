#!/usr/bin/env bash
# make-test-ca.sh — a certificate authority that exists for one test run.
#
# The clients validate certificates, and they should: an app that
# accepts any certificate a machine on the path can produce has thrown
# away the whole of the protection. So a test cannot reach a TLS mail
# server by relaxing the app — it reaches one by giving the *device* a
# root it trusts, which is what a simulator's keychain and Android's
# `debug-overrides` are for. Neither touches a release build.
#
# Generated per run rather than committed: a private key in git is a
# private key in git, even a throwaway one, and this takes a second.
#
# Usage: ./make-test-ca.sh <out-dir>
#   writes ca.pem, ca.key, server.pem, server.key, server-chain.pem
set -euo pipefail
OUT="${1:?usage: make-test-ca.sh <out-dir>}"
mkdir -p "$OUT"
cd "$OUT"

# The CA.
openssl req -x509 -newkey rsa:2048 -sha256 -days 2 -nodes \
    -keyout ca.key -out ca.pem \
    -subj "/CN=mailrs test CA/O=mailrs tests" \
    -addext "basicConstraints=critical,CA:TRUE" \
    -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null

# The server it signs. Both names, because a simulator reaches the host
# as `localhost` and an Android emulator reaches it as 10.0.2.2.
openssl req -newkey rsa:2048 -sha256 -nodes \
    -keyout server.key -out server.csr \
    -subj "/CN=localhost" 2>/dev/null

cat > server.ext <<'EXT'
basicConstraints=CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
subjectAltName=DNS:localhost,IP:127.0.0.1,IP:10.0.2.2
EXT

openssl x509 -req -in server.csr -CA ca.pem -CAkey ca.key -CAcreateserial \
    -out server.pem -days 2 -sha256 -extfile server.ext 2>/dev/null

cat server.pem ca.pem > server-chain.pem

# A second authority, deliberately **never installed anywhere**.
#
# It exists so the suite can keep a standing assertion that a
# certificate the device does not trust is an *error* — not a wait.
# That distinction was not free: the client sat in `NWConnection`'s
# `.waiting` state forever when a handshake was refused, which is an
# app that hangs exactly for the people whose network is being
# interfered with. A test that needs somebody to hand-edit the build
# script is a test that runs once.
openssl req -x509 -newkey rsa:2048 -sha256 -days 2 -nodes \
    -keyout rogue-ca.key -out rogue-ca.pem \
    -subj "/CN=nobody's CA/O=untrusted" \
    -addext "basicConstraints=critical,CA:TRUE" \
    -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null

openssl req -newkey rsa:2048 -sha256 -nodes \
    -keyout rogue.key -out rogue.csr -subj "/CN=localhost" 2>/dev/null

openssl x509 -req -in rogue.csr -CA rogue-ca.pem -CAkey rogue-ca.key -CAcreateserial \
    -out rogue.pem -days 2 -sha256 -extfile server.ext 2>/dev/null

cat rogue.pem rogue-ca.pem > rogue-chain.pem

rm -f server.csr rogue.csr server.ext ca.srl rogue-ca.srl
