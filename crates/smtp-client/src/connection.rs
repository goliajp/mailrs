use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use rustls::ClientConfig;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufStream, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

use crate::mx::{format_mail_from, format_rcpt_to};
use crate::response::{parse_response, SmtpResponse};

/// connection timeout configuration
#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    pub connect: std::time::Duration,
    pub greeting: std::time::Duration,
    pub command: std::time::Duration,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            connect: std::time::Duration::from_secs(30),
            greeting: std::time::Duration::from_secs(30),
            command: std::time::Duration::from_secs(60),
        }
    }
}

enum Transport {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for Transport {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Transport::Plain(s) => Pin::new(s).poll_read(cx, buf),
            Transport::Tls(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Transport {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Transport::Plain(s) => Pin::new(s).poll_write(cx, buf),
            Transport::Tls(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Transport::Plain(s) => Pin::new(s).poll_flush(cx),
            Transport::Tls(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Transport::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Transport::Tls(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

/// SMTP client connection for outbound delivery
pub struct SmtpConnection {
    stream: BufStream<Transport>,
    command_timeout: std::time::Duration,
}

impl SmtpConnection {
    /// connect to an SMTP server and read the greeting
    pub async fn connect(host: &str, port: u16) -> io::Result<Self> {
        Self::connect_with_timeout(host, port, &TimeoutConfig::default()).await
    }

    /// connect with explicit timeout configuration
    pub async fn connect_with_timeout(
        host: &str,
        port: u16,
        timeouts: &TimeoutConfig,
    ) -> io::Result<Self> {
        let tcp = tokio::time::timeout(timeouts.connect, TcpStream::connect((host, port)))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "connect timeout"))??;

        let mut conn = Self {
            stream: BufStream::new(Transport::Plain(tcp)),
            command_timeout: timeouts.command,
        };

        let greeting = tokio::time::timeout(timeouts.greeting, conn.read_response())
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "greeting timeout"))??;

        if !greeting.is_positive() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                format!("server rejected: {}", greeting.message()),
            ));
        }
        Ok(conn)
    }

    /// send EHLO and return the response
    pub async fn ehlo(&mut self, hostname: &str) -> io::Result<SmtpResponse> {
        self.send_command(&format!("EHLO {hostname}\r\n")).await
    }

    /// upgrade to TLS via STARTTLS
    pub async fn starttls(mut self, hostname: &str) -> io::Result<Self> {
        let resp = self.send_command("STARTTLS\r\n").await?;
        if !resp.is_positive() {
            return Err(io::Error::other(format!(
                "STARTTLS rejected: {}",
                resp.message()
            )));
        }

        let mut config = ClientConfig::builder()
            .with_root_certificates(rustls::RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            })
            .with_no_client_auth();
        config.alpn_protocols = vec![];

        let connector = TlsConnector::from(Arc::new(config));
        let server_name: rustls::pki_types::ServerName<'static> =
            hostname.to_string().try_into().map_err(|e| {
                io::Error::new(io::ErrorKind::InvalidInput, format!("invalid SNI: {e}"))
            })?;

        // extract inner TCP stream
        let inner = self.stream.into_inner();
        let tcp = match inner {
            Transport::Plain(tcp) => tcp,
            Transport::Tls(_) => {
                return Err(io::Error::other("already using TLS"));
            }
        };

        let tls_stream = connector.connect(server_name, tcp).await?;
        Ok(Self {
            stream: BufStream::new(Transport::Tls(Box::new(tls_stream))),
            command_timeout: self.command_timeout,
        })
    }

    /// send MAIL FROM, RCPT TO, DATA, and message body
    pub async fn deliver(
        &mut self,
        from: &str,
        to: &[&str],
        message: &[u8],
    ) -> io::Result<SmtpResponse> {
        // MAIL FROM
        let resp = self.send_command(&format_mail_from(from)).await?;
        if !resp.is_positive() {
            return Ok(resp);
        }

        // RCPT TO
        for recipient in to {
            let resp = self.send_command(&format_rcpt_to(recipient)).await?;
            if !resp.is_positive() {
                return Ok(resp);
            }
        }

        // DATA
        let resp = self.send_command("DATA\r\n").await?;
        if resp.code != 354 {
            return Ok(resp);
        }

        // send message body with dot-stuffing (RFC 5321 section 4.5.2)
        let stuffed = dot_stuff(message);
        self.stream.write_all(&stuffed).await?;
        if !stuffed.ends_with(b"\r\n") {
            self.stream.write_all(b"\r\n").await?;
        }
        self.stream.write_all(b".\r\n").await?;
        self.stream.flush().await?;

        tokio::time::timeout(self.command_timeout, self.read_response())
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "DATA response timeout"))?
    }

    /// send QUIT
    pub async fn quit(&mut self) -> io::Result<()> {
        let _ = self.send_command("QUIT\r\n").await;
        Ok(())
    }

    async fn send_command(&mut self, cmd: &str) -> io::Result<SmtpResponse> {
        self.stream.write_all(cmd.as_bytes()).await?;
        self.stream.flush().await?;
        tokio::time::timeout(self.command_timeout, self.read_response())
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "command timeout"))?
    }

    async fn read_response(&mut self) -> io::Result<SmtpResponse> {
        const MAX_RESPONSE_SIZE: usize = 65536;
        let mut buf = String::new();
        loop {
            let mut line = String::new();
            let n = self.stream.read_line(&mut line).await?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "connection closed",
                ));
            }
            buf.push_str(&line);
            if buf.len() > MAX_RESPONSE_SIZE {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "SMTP response too large",
                ));
            }

            // check if this is the final line (code followed by space)
            if line.len() >= 4 && line.as_bytes()[3] == b' ' {
                break;
            }
        }
        parse_response(&buf).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid SMTP response: {buf}"),
            )
        })
    }
}

/// dot-stuff message body for SMTP DATA transmission (RFC 5321 section 4.5.2)
/// lines starting with '.' get an extra '.' prepended
pub fn dot_stuff(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len());
    let mut at_line_start = true;

    for &byte in data {
        if at_line_start && byte == b'.' {
            result.push(b'.');
        }
        result.push(byte);
        at_line_start = byte == b'\n';
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_stuff_no_dots() {
        assert_eq!(dot_stuff(b"hello\r\nworld\r\n"), b"hello\r\nworld\r\n");
    }

    #[test]
    fn dot_stuff_line_starting_with_dot() {
        assert_eq!(dot_stuff(b".hello\r\n"), b"..hello\r\n");
    }

    #[test]
    fn dot_stuff_multiple_dots() {
        assert_eq!(
            dot_stuff(b"ok\r\n.line1\r\n..line2\r\n"),
            b"ok\r\n..line1\r\n...line2\r\n"
        );
    }

    #[test]
    fn dot_stuff_dot_only_line() {
        // a lone dot on a line would be end-of-data marker without stuffing
        assert_eq!(dot_stuff(b".\r\n"), b"..\r\n");
    }

    #[test]
    fn dot_stuff_empty() {
        assert_eq!(dot_stuff(b""), b"");
    }

    #[test]
    fn timeout_config_defaults() {
        let cfg = TimeoutConfig::default();
        assert_eq!(cfg.connect, std::time::Duration::from_secs(30));
        assert_eq!(cfg.greeting, std::time::Duration::from_secs(30));
        assert_eq!(cfg.command, std::time::Duration::from_secs(60));
    }
}
