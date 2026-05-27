//! In-process mock SMTP server for `mailrs-smtp-client` integration tests.
//!
//! Spins up a `tokio::net::TcpListener` on an ephemeral port and serves one
//! configurable [`Behavior`] per connection. Behaviors cover the path
//! taxonomy `SmtpConnection` needs:
//!
//! - happy path (EHLO advertises STARTTLS or not, MAIL/RCPT/DATA accepted)
//! - explicit rejection (5xx after MAIL, 5xx after RCPT, 4xx after RCPT)
//! - connection-level failure (hang at greeting, close mid-DATA)
//! - STARTTLS-specific (250 OK then TLS handshake fails by closing)
//! - STARTTLS happy path (server-side rustls with an ephemeral self-signed cert)
//!
//! The mock is deliberately not a real SMTP server. It implements the
//! minimum command-response interleaving the client cares about; it does
//! NOT enforce RFC 5321 ordering, doesn't pipeline, and ignores anything
//! it doesn't recognise. Tests are responsible for driving the right
//! command sequence for the behavior they configured.

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// Per-connection behavior the mock should emulate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behavior {
    /// Full happy path: 220 greeting, EHLO advertises STARTTLS +
    /// PIPELINING, MAIL/RCPT/DATA accepted, QUIT 221.
    Accept,
    /// Same as `Accept` but EHLO does NOT advertise STARTTLS.
    AcceptNoStarttls,
    /// MAIL FROM is rejected with `550`. Subsequent commands return
    /// `503 bad sequence`.
    Reject5xxAfterMail,
    /// MAIL FROM accepted, RCPT TO rejected with `550`.
    Reject5xxAfterRcpt,
    /// MAIL FROM accepted, RCPT TO deferred with `450`.
    Defer4xxAfterRcpt,
    /// TCP connects, server reads the request but never writes the
    /// 220 greeting. Drives the client's greeting-timeout path.
    HangAfterConnect,
    /// `DATA` returns `354`; on receiving the next byte from the
    /// client, server closes the socket abruptly (unexpected EOF
    /// while the body is being sent).
    CloseMidData,
    /// `STARTTLS` returns `220 Ready` then server closes the TCP
    /// socket immediately, so the client's rustls handshake fails.
    StarttlsHandshakeFail,
    /// EHLO is advertised normally (with STARTTLS) but the
    /// STARTTLS command itself is rejected with `502 not
    /// implemented`. Drives the `StarttlsResult::Rejected` branch
    /// of the worker.
    StarttlsRejected,
    /// First command after greeting (EHLO) is rejected with a 500.
    /// Drives the `EHLO rejected` early-return in the worker.
    EhloRejected,
    /// `STARTTLS` returns `220 Ready` then the server completes a
    /// real rustls server-side handshake with a fresh self-signed
    /// cert. The client must be configured with a verifier that
    /// accepts this cert (see `tests/connection_integration.rs`).
    StarttlsAccept,
}

/// Handle returned by [`spawn_mock_smtp`]. Dropping the handle does NOT
/// stop the listener (the spawned task owns it); on test shutdown the
/// process exit cleans up. The bound `addr` is the only thing tests
/// need to connect.
pub struct MockHandle {
    pub addr: SocketAddr,
    pub task: JoinHandle<()>,
}

/// Install rustls's `ring` provider as the process-level default if
/// nothing else has yet. Production sets this from `server::main`;
/// integration tests don't go through main, so any test that touches
/// rustls (mock TLS server, client STARTTLS handshake) needs to call
/// this first. Idempotent — repeated calls and races are harmless.
pub fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Spin up a mock SMTP server on `127.0.0.1:0` (ephemeral port) that
/// serves connections with the configured [`Behavior`] (one behavior
/// per accept; the listener loops so reconnects are handled too).
/// Returns the bound address + task handle.
pub async fn spawn_mock_smtp(behavior: Behavior) -> MockHandle {
    ensure_crypto_provider();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock smtp listener");
    let addr = listener.local_addr().expect("local_addr");

    let task = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _peer)) => {
                    let b = behavior;
                    tokio::spawn(async move { handle_connection(stream, b).await });
                }
                Err(_) => return,
            }
        }
    });

    MockHandle { addr, task }
}

async fn handle_connection(stream: TcpStream, behavior: Behavior) {
    if behavior == Behavior::HangAfterConnect {
        // hold the connection open with no data flowing — drives the
        // client's greeting-timeout path
        let _hold = stream;
        std::future::pending::<()>().await;
        unreachable!()
    }

    let (rh, mut wh) = stream.into_split();
    let mut reader = BufReader::new(rh);

    if wh
        .write_all(b"220 mock.test ESMTP ready\r\n")
        .await
        .is_err()
    {
        return;
    }

    let mut after_mail = false;

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line).await {
            Ok(0) => return,
            Ok(_) => {}
            Err(_) => return,
        }
        let upper = line.trim_end_matches(['\r', '\n']).to_uppercase();

        if upper.starts_with("EHLO") || upper.starts_with("HELO") {
            if behavior == Behavior::EhloRejected {
                let _ = wh.write_all(b"500 syntax error\r\n").await;
                continue;
            }
            let resp = match behavior {
                Behavior::AcceptNoStarttls => "250-mock.test hi\r\n250 PIPELINING\r\n",
                _ => "250-mock.test hi\r\n250-STARTTLS\r\n250 PIPELINING\r\n",
            };
            if wh.write_all(resp.as_bytes()).await.is_err() {
                return;
            }
        } else if upper.starts_with("STARTTLS") {
            match behavior {
                Behavior::StarttlsRejected => {
                    let _ = wh.write_all(b"502 not implemented\r\n").await;
                }
                Behavior::StarttlsHandshakeFail => {
                    let _ = wh.write_all(b"220 Ready\r\n").await;
                    let _ = wh.flush().await;
                    drop(wh);
                    drop(reader);
                    return;
                }
                Behavior::StarttlsAccept => {
                    let _ = wh.write_all(b"220 Ready\r\n").await;
                    let _ = wh.flush().await;
                    // hand off the (still-plaintext) socket to a TLS
                    // upgrade routine
                    let tcp = match reader.into_inner().reunite(wh) {
                        Ok(t) => t,
                        Err(_) => return,
                    };
                    if let Err(e) = handle_starttls(tcp).await {
                        eprintln!("mock starttls upgrade failed: {e}");
                    }
                    return;
                }
                _ => {
                    let _ = wh.write_all(b"502 not implemented\r\n").await;
                }
            }
        } else if upper.starts_with("MAIL FROM") {
            match behavior {
                Behavior::Reject5xxAfterMail => {
                    let _ = wh.write_all(b"550 mailbox unavailable\r\n").await;
                }
                _ => {
                    let _ = wh.write_all(b"250 OK\r\n").await;
                    after_mail = true;
                }
            }
        } else if upper.starts_with("RCPT TO") {
            if !after_mail {
                let _ = wh.write_all(b"503 need MAIL FROM first\r\n").await;
                continue;
            }
            match behavior {
                Behavior::Reject5xxAfterRcpt => {
                    let _ = wh.write_all(b"550 user unknown\r\n").await;
                }
                Behavior::Defer4xxAfterRcpt => {
                    let _ = wh.write_all(b"450 try again later\r\n").await;
                }
                _ => {
                    let _ = wh.write_all(b"250 OK\r\n").await;
                }
            }
        } else if upper.starts_with("DATA") {
            let _ = wh.write_all(b"354 end with .\r\n").await;
            let _ = wh.flush().await;

            if behavior == Behavior::CloseMidData {
                // wait for the client to start sending body, then
                // bail before they finish
                let mut sink = [0u8; 16];
                let _ = reader.read(&mut sink).await;
                drop(wh);
                drop(reader);
                return;
            }

            // consume body until end-of-data marker (CRLF.CRLF)
            let mut prev_line_was_empty_after_data = true;
            loop {
                let mut body_line = String::new();
                match reader.read_line(&mut body_line).await {
                    Ok(0) => return,
                    Ok(_) => {}
                    Err(_) => return,
                }
                if body_line == ".\r\n" && prev_line_was_empty_after_data {
                    let _ = wh.write_all(b"250 2.0.0 OK queued\r\n").await;
                    break;
                }
                if body_line == ".\r\n" {
                    let _ = wh.write_all(b"250 2.0.0 OK queued\r\n").await;
                    break;
                }
                prev_line_was_empty_after_data = body_line == "\r\n";
            }
        } else if upper.starts_with("QUIT") {
            let _ = wh.write_all(b"221 bye\r\n").await;
            return;
        } else if upper.is_empty() {
            return;
        } else {
            let _ = wh.write_all(b"502 unrecognised command\r\n").await;
        }
    }
}

async fn handle_starttls(tcp: TcpStream) -> std::io::Result<()> {
    use tokio_rustls::TlsAcceptor;

    let (cert, key) = make_self_signed_cert();
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .map_err(|e| std::io::Error::other(format!("tls config: {e}")))?;
    let acceptor = TlsAcceptor::from(Arc::new(config));

    let mut tls = acceptor.accept(tcp).await?;

    // post-STARTTLS, expect a fresh EHLO from the client, then accept
    // a minimal MAIL/RCPT/DATA cycle so the integration test can drive
    // an end-to-end happy path on TLS.
    use tokio::io::AsyncBufReadExt as _;
    let (rh, mut wh) = tokio::io::split(&mut tls);
    let mut reader = BufReader::new(rh);

    let mut after_mail = false;
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line).await {
            Ok(0) => return Ok(()),
            Ok(_) => {}
            Err(_) => return Ok(()),
        }
        let upper = line.trim_end_matches(['\r', '\n']).to_uppercase();

        if upper.starts_with("EHLO") || upper.starts_with("HELO") {
            wh.write_all(b"250-mock.test tls\r\n250 PIPELINING\r\n").await?;
        } else if upper.starts_with("MAIL FROM") {
            wh.write_all(b"250 OK\r\n").await?;
            after_mail = true;
        } else if upper.starts_with("RCPT TO") {
            if !after_mail {
                wh.write_all(b"503 need MAIL FROM\r\n").await?;
                continue;
            }
            wh.write_all(b"250 OK\r\n").await?;
        } else if upper.starts_with("DATA") {
            wh.write_all(b"354 end with .\r\n").await?;
            wh.flush().await?;
            loop {
                let mut body = String::new();
                match reader.read_line(&mut body).await {
                    Ok(0) => return Ok(()),
                    Ok(_) => {}
                    Err(_) => return Ok(()),
                }
                if body == ".\r\n" {
                    wh.write_all(b"250 2.0.0 OK queued\r\n").await?;
                    break;
                }
            }
        } else if upper.starts_with("QUIT") {
            wh.write_all(b"221 bye\r\n").await?;
            return Ok(());
        }
    }
}

/// `rustls::ClientConfig` that accepts ANY server certificate without
/// validation. ONLY safe inside tests against the in-process mock SMTP
/// server — never use in production. Used to drive the
/// STARTTLS-success path of `mailrs-outbound-queue::worker::try_deliver_via_mx_with_tls`
/// when the mock presents a fresh self-signed cert that no real CA
/// could possibly trust.
pub fn skip_verify_client_config() -> rustls::ClientConfig {
    use rustls::DigitallySignedStruct;
    use rustls::SignatureScheme;
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};

    #[derive(Debug)]
    struct NoVerify;

    impl ServerCertVerifier for NoVerify {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp_response: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            vec![
                SignatureScheme::RSA_PKCS1_SHA256,
                SignatureScheme::RSA_PKCS1_SHA384,
                SignatureScheme::RSA_PKCS1_SHA512,
                SignatureScheme::ECDSA_NISTP256_SHA256,
                SignatureScheme::ECDSA_NISTP384_SHA384,
                SignatureScheme::ED25519,
                SignatureScheme::RSA_PSS_SHA256,
                SignatureScheme::RSA_PSS_SHA384,
                SignatureScheme::RSA_PSS_SHA512,
            ]
        }
    }

    ensure_crypto_provider();
    rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerify))
        .with_no_client_auth()
}

/// Generate a fresh self-signed cert + private key for the mock TLS
/// server. The cert SAN is `mock.test` so a custom rustls verifier in
/// the test client can match on that.
pub fn make_self_signed_cert() -> (
    rustls::pki_types::CertificateDer<'static>,
    rustls::pki_types::PrivateKeyDer<'static>,
) {
    let cert = rcgen::generate_simple_self_signed(vec!["mock.test".to_string()])
        .expect("rcgen self-signed");
    let cert_der = cert.cert.der().clone();
    let key_der = rustls::pki_types::PrivateKeyDer::Pkcs8(cert.signing_key.serialize_der().into());
    (cert_der, key_der)
}
