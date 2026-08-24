#!/usr/bin/env python3
"""A mail server that exists so a test can reach one over real TLS.

Every assertion in this repo about IMAP and SMTP is made against a
*scripted transport* — a fake that hands the session lines from a list.
That covers the conversation and nothing under it: the TLS handshake,
the certificate check, the socket's own framing, and on iOS the
`UpgradableTransport` that STARTTLS needs. Those had never been run at
all, on either platform, and they are exactly the parts that cannot be
made to misbehave on demand by a real provider either.

So: a small server that speaks enough IMAP and SMTP to sync a folder
and accept a message, wrapped in TLS with a certificate the *device*
has been told to trust. The app is not modified and validates the
certificate as it always does — see `make-test-ca.sh` for why that
distinction is the whole point.

Two ports:
  993  implicit TLS IMAP
  465  implicit TLS SMTP
  587  plaintext SMTP offering STARTTLS   <- the path with no coverage
  994  implicit TLS IMAP with a certificate **nobody trusts**
  995  plain HTTP, one route, saying what actually arrived

Usage:
  tls-mail-stub.py <cert-dir> [--imaps N] [--smtps N] [--submission N]
"""

import argparse
import socket
import ssl
import sys
import threading

# One account, one folder, two messages. Small on purpose: this is here
# to prove the wire works end to end, not to re-test the parsers, which
# have their own suites and awkward cases.
USER = "me@example.com"
PASSWORD = "app-password"

MESSAGES = [
    (
        1001,
        b"From: Ada <ada@example.com>\r\n"
        b"Subject: =?utf-8?B?5Lya6K2w?=\r\n"
        b"Date: Sun, 24 Aug 2025 01:46:40 +0000\r\n"
        b"Message-ID: <m1001@example.com>\r\n\r\n",
    ),
    (
        1002,
        b"From: Bob <bob@example.com>\r\n"
        b"Subject: Lunch\r\n"
        b"Date: Sun, 24 Aug 2025 02:46:40 +0000\r\n"
        b"Message-ID: <m1002@example.com>\r\n\r\n",
    ),
]

UIDVALIDITY = 42

# What arrived, so a test can assert the message really crossed a
# socket rather than that a call returned.
received = []
received_lock = threading.Lock()


def context(cert_dir, rogue=False):
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    if rogue:
        # Signed by an authority that is installed nowhere, so a client
        # reaching this port *should* refuse it. What is being tested
        # is that refusing is an error and not a wait.
        ctx.load_cert_chain(f"{cert_dir}/rogue-chain.pem", f"{cert_dir}/rogue.key")
    else:
        ctx.load_cert_chain(f"{cert_dir}/server-chain.pem", f"{cert_dir}/server.key")
    return ctx


class Line:
    """Read CRLF-terminated lines off a socket, keeping the remainder."""

    def __init__(self, conn):
        self.conn = conn
        self.buf = b""

    def read(self):
        while b"\r\n" not in self.buf:
            chunk = self.conn.recv(4096)
            if not chunk:
                return None
            self.buf += chunk
        line, self.buf = self.buf.split(b"\r\n", 1)
        return line.decode("latin-1")

    def read_bytes(self, count):
        while len(self.buf) < count:
            chunk = self.conn.recv(4096)
            if not chunk:
                break
            self.buf += chunk
        out, self.buf = self.buf[:count], self.buf[count:]
        return out


def imap_session(conn):
    line = Line(conn)
    conn.sendall(b"* OK [CAPABILITY IMAP4rev1 MOVE UIDPLUS] mailrs test stub\r\n")
    selected = False
    while True:
        raw = line.read()
        if raw is None:
            return
        parts = raw.split(" ", 2)
        if len(parts) < 2:
            continue
        tag, verb = parts[0], parts[1].upper()
        rest = parts[2] if len(parts) > 2 else ""

        if verb == "CAPABILITY":
            conn.sendall(b"* CAPABILITY IMAP4rev1 MOVE UIDPLUS\r\n")
            conn.sendall(f"{tag} OK done\r\n".encode())
        elif verb == "LOGIN":
            # `LOGIN "user" "pass"` — quoted by the client.
            got = rest.replace('"', "").split(" ")
            if len(got) >= 2 and got[0] == USER and got[1] == PASSWORD:
                conn.sendall(f"{tag} OK signed in\r\n".encode())
            else:
                conn.sendall(f"{tag} NO [AUTHENTICATIONFAILED] no\r\n".encode())
        elif verb == "LIST":
            conn.sendall(b'* LIST (\\HasNoChildren) "." "INBOX"\r\n')
            conn.sendall(f"{tag} OK done\r\n".encode())
        elif verb == "SELECT":
            selected = True
            conn.sendall(f"* {len(MESSAGES)} EXISTS\r\n".encode())
            conn.sendall(f"* OK [UIDVALIDITY {UIDVALIDITY}] valid\r\n".encode())
            conn.sendall(f"* OK [UIDNEXT 1003] next\r\n".encode())
            conn.sendall(f"{tag} OK [READ-WRITE] selected\r\n".encode())
        elif verb == "FETCH" or (verb == "UID" and rest.upper().startswith("FETCH")):
            # **Both spellings.** A first pass counts from the end of
            # the folder and asks by *sequence number* (`FETCH 1:*`);
            # only a pass that already has a uid asks `UID FETCH`. A
            # stub answering one of them and a bare OK to the other
            # says "fetched nothing" to the case that actually happens
            # first — which is what it did, and the client had no way
            # to tell that from an empty mailbox.
            if selected:
                for index, (uid, body) in enumerate(MESSAGES, start=1):
                    conn.sendall(
                        f"* {index} FETCH (UID {uid} FLAGS () "
                        f"RFC822.SIZE {len(body)} "
                        f"BODY[HEADER] {{{len(body)}}}\r\n".encode()
                    )
                    conn.sendall(body + b")\r\n")
            conn.sendall(f"{tag} OK fetched\r\n".encode())
        elif verb == "STORE" or (verb == "UID" and rest.upper().startswith("STORE")):
            conn.sendall(f"{tag} OK stored\r\n".encode())
        elif verb == "NOOP":
            conn.sendall(f"{tag} OK done\r\n".encode())
        elif verb == "LOGOUT":
            conn.sendall(b"* BYE\r\n")
            conn.sendall(f"{tag} OK done\r\n".encode())
            return
        else:
            conn.sendall(f"{tag} OK done\r\n".encode())


def smtp_session(conn, ctx=None):
    """`ctx` set means plaintext-with-STARTTLS; None means already TLS."""
    line = Line(conn)
    conn.sendall(b"220 mailrs test stub ESMTP\r\n")
    upgraded = ctx is None
    body = None
    while True:
        raw = line.read()
        if raw is None:
            return
        verb = raw.split(" ")[0].upper()
        if verb == "EHLO":
            conn.sendall(b"250-mailrs test stub\r\n")
            conn.sendall(b"250-SIZE 35882577\r\n")
            if not upgraded:
                conn.sendall(b"250-STARTTLS\r\n")
            conn.sendall(b"250 AUTH PLAIN LOGIN\r\n")
        elif verb == "STARTTLS":
            conn.sendall(b"220 go ahead\r\n")
            conn = ctx.wrap_socket(conn, server_side=True)
            line = Line(conn)
            upgraded = True
        elif verb == "AUTH":
            # Only after TLS. A stub that accepted a credential in the
            # clear would let a downgrade pass this suite.
            if not upgraded:
                conn.sendall(b"538 encryption required\r\n")
            else:
                conn.sendall(b"235 accepted\r\n")
        elif verb in ("MAIL", "RCPT"):
            conn.sendall(b"250 ok\r\n")
        elif verb == "DATA":
            conn.sendall(b"354 go ahead\r\n")
            body = b""
            while True:
                chunk = line.read()
                if chunk is None:
                    return
                if chunk == ".":
                    break
                # Un-stuff, so what is recorded is what was sent.
                if chunk.startswith(".."):
                    chunk = chunk[1:]
                body += chunk.encode("latin-1") + b"\r\n"
            with received_lock:
                received.append(body)
            conn.sendall(b"250 2.0.0 queued\r\n")
        elif verb == "QUIT":
            conn.sendall(b"221 bye\r\n")
            return
        else:
            conn.sendall(b"250 ok\r\n")


class Probe(threading.Thread):
    """A plain-HTTP window onto what the mail stub received.

    A test that asserts "the compose sheet closed" is asserting about
    the sheet. What has to be true is that a message **crossed the
    socket**, and the only place that is knowable is here. One route,
    no arguments:

        GET /received   ->   {"count": N, "last": "<the DATA block>"}
    """

    def __init__(self, port):
        super().__init__(daemon=True)
        self.port = port

    def run(self):
        import http.server
        import json as jsonlib

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                with received_lock:
                    body = jsonlib.dumps(
                        {
                            "count": len(received),
                            "last": received[-1].decode("latin-1") if received else "",
                        }
                    ).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *args):
                pass

        http.server.HTTPServer(("0.0.0.0", self.port), Handler).serve_forever()


def serve(port, handler, ctx, wrap):
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind(("0.0.0.0", port))
    server.listen(8)
    while True:
        conn, _ = server.accept()
        threading.Thread(
            target=guarded, args=(conn, handler, ctx, wrap), daemon=True
        ).start()


def guarded(conn, handler, ctx, wrap):
    try:
        if wrap:
            conn = ctx.wrap_socket(conn, server_side=True)
            handler(conn)
        else:
            handler(conn, ctx)
    except Exception as e:  # a test client hanging up is not a fault
        print(f"stub: {type(e).__name__}: {e}", file=sys.stderr, flush=True)
    finally:
        try:
            conn.close()
        except OSError:
            pass


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cert_dir")
    ap.add_argument("--imaps", type=int, default=9993)
    ap.add_argument("--smtps", type=int, default=9465)
    ap.add_argument("--submission", type=int, default=9587)
    ap.add_argument("--imaps-untrusted", type=int, default=9994)
    ap.add_argument("--probe", type=int, default=9995)
    args = ap.parse_args()

    ctx = context(args.cert_dir)
    rogue = context(args.cert_dir, rogue=True)
    for port, handler, wrap, which in (
        (args.imaps, imap_session, True, ctx),
        (args.smtps, smtp_session, True, ctx),
        (args.submission, smtp_session, False, ctx),
        (args.imaps_untrusted, imap_session, True, rogue),
    ):
        threading.Thread(
            target=serve, args=(port, handler, which, wrap), daemon=True
        ).start()
    Probe(args.probe).start()
    print(
        f"tls-mail-stub: imaps={args.imaps} smtps={args.smtps} "
        f"submission={args.submission} imaps-untrusted={args.imaps_untrusted} "
        f"probe={args.probe}",
        flush=True,
    )
    threading.Event().wait()


if __name__ == "__main__":
    main()
