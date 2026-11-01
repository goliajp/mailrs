//! Reaching the server an import reads from.
//!
//! The host is the user's to name, so this is a connection made on their
//! behalf to anywhere: every address the name resolves to must be on the
//! public internet (`mailrs_fetch::address::public`), and the connection
//! goes to the address that was checked, so a second lookup cannot answer
//! differently. Only 993 (TLS from the start) and 143 (upgraded with
//! STARTTLS, or abandoned) are offered, the certificate is verified
//! against the anchors outbound mail uses, and there is no switch to skip
//! that.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// A stream an IMAP session runs over.
pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

/// An open connection, and whether its greeting was already read.
pub struct Opened {
    pub stream: Box<dyn Stream>,
    pub greeted: bool,
}

/// How an import reaches its server. Tests hand in their own; the import
/// itself only ever uses [`Checked`].
pub trait Connect: Send + Sync {
    fn connect<'a>(
        &'a self,
        host: &'a str,
        port: u16,
    ) -> Pin<Box<dyn Future<Output = Result<Opened, String>> + Send + 'a>>;
}

/// The ports offered: 993, TLS from the start; 143, upgraded with STARTTLS.
pub const PORTS: [u16; 2] = [993, 143];

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Public addresses only, TLS always.
pub struct Checked;

impl Connect for Checked {
    fn connect<'a>(
        &'a self,
        host: &'a str,
        port: u16,
    ) -> Pin<Box<dyn Future<Output = Result<Opened, String>> + Send + 'a>> {
        Box::pin(async move {
            tokio::time::timeout(CONNECT_TIMEOUT, open(host, port))
                .await
                .map_err(|_| format!("{host} did not answer in time"))?
        })
    }
}

/// Every address `host` resolves to, refused unless all are public.
pub async fn public_addresses(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| format!("{host} could not be found"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("{host} could not be found"));
    }
    for a in &addrs {
        mailrs_fetch::address::public(a.ip()).map_err(|e| format!("{host}: {e}"))?;
    }
    Ok(addrs)
}

async fn open(host: &str, port: u16) -> Result<Opened, String> {
    if !PORTS.contains(&port) {
        return Err("only ports 993 and 143".into());
    }
    let addrs = public_addresses(host, port).await?;
    let tcp = TcpStream::connect(addrs[0])
        .await
        .map_err(|e| format!("{host}: {e}"))?;
    match port {
        993 => Ok(Opened {
            stream: Box::new(tls(host, tcp).await?),
            greeted: false,
        }),
        _ => {
            let tcp = starttls(tcp).await?;
            Ok(Opened {
                stream: Box::new(tls(host, tcp).await?),
                greeted: true,
            })
        }
    }
}

async fn tls(
    host: &str,
    tcp: TcpStream,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>, String> {
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(mailrs_smtp_client::pkix_root_store())
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|_| format!("{host} is not a name a certificate can carry"))?;
    tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(name, tcp)
        .await
        .map_err(|e| format!("{host}: TLS failed: {e}"))
}

/// One line of the plain-text exchange before TLS, bounded.
async fn short_line(r: &mut BufReader<&mut TcpStream>) -> Result<String, String> {
    let mut line = String::new();
    let mut limited = (&mut *r).take(4096);
    limited
        .read_line(&mut line)
        .await
        .map_err(|e| e.to_string())?;
    if !line.ends_with('\n') {
        return Err("the server's greeting was not a line".into());
    }
    Ok(line.trim_end().to_string())
}

/// Greeting, `STARTTLS`, its `OK` — then the bare connection, ready for
/// the handshake. Anything else and the import does not go ahead in clear.
async fn starttls(mut tcp: TcpStream) -> Result<TcpStream, String> {
    {
        let mut r = BufReader::new(&mut tcp);
        let greeting = short_line(&mut r).await?;
        if !greeting.starts_with("* OK") {
            return Err(format!(
                "the server refused: {}",
                super::client::shown(&greeting)
            ));
        }
    }
    tcp.write_all(b"s0 STARTTLS\r\n")
        .await
        .map_err(|e| e.to_string())?;
    let mut r = BufReader::new(&mut tcp);
    for _ in 0..16 {
        let line = short_line(&mut r).await?;
        if let Some(rest) = line.strip_prefix("s0 ") {
            if rest.to_ascii_uppercase().starts_with("OK") {
                return Ok(tcp);
            }
            return Err(format!(
                "the server would not start TLS: {}",
                super::client::shown(rest)
            ));
        }
    }
    Err("the server would not start TLS".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_name_for_a_private_address_is_refused() {
        for host in ["127.0.0.1", "localhost", "10.1.2.3", "169.254.169.254"] {
            let e = public_addresses(host, 993).await.unwrap_err();
            assert!(e.contains("not a public address"), "{host}: {e}");
        }
    }

    #[tokio::test]
    async fn only_the_two_imap_ports_are_offered() {
        let e = Checked.connect("imap.example.com", 25).await.err().unwrap();
        assert_eq!(e, "only ports 993 and 143");
    }
}
