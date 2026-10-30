//! Fetching one remote image for a message, safely, and handing back a
//! re-encoded copy with nothing of the original's metadata.
//!
//! Every hop is checked before it is connected: the scheme and port, then
//! every address the host resolves to, then the connection is made to the
//! address that was checked, so a second lookup cannot answer differently
//! (DNS rebinding). Redirects are followed by hand, a few at most, each
//! checked the same way. Only an image of bounded size and dimensions comes
//! back, decoded under a memory cap and encoded afresh.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

pub mod address;
mod reencode;

pub use address::Refused;
pub use reencode::{Image, reencode};

/// The bounds of one fetch.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_bytes: usize,
    pub timeout: Duration,
    pub max_redirects: u8,
    /// Widest and tallest image decoded, checked before any pixel buffer
    /// is allocated.
    pub max_side: u32,
}

impl Limits {
    pub const DEFAULT: Self = Self {
        max_bytes: 5 * 1024 * 1024,
        timeout: Duration::from_secs(10),
        max_redirects: 3,
        max_side: 4096,
    };
}

/// Why nothing came back.
#[derive(Debug)]
pub enum Failed {
    Refused(Refused),
    TooManyRedirects,
    Status(u16),
    NotAnImage(String),
    TooLarge,
    Unreadable(String),
    Network(String),
}

impl std::fmt::Display for Failed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failed::Refused(r) => write!(f, "refused: {r}"),
            Failed::TooManyRedirects => write!(f, "too many redirects"),
            Failed::Status(s) => write!(f, "answered {s}"),
            Failed::NotAnImage(t) => write!(f, "not an image: {t}"),
            Failed::TooLarge => write!(f, "larger than allowed"),
            Failed::Unreadable(e) => write!(f, "not a readable image: {e}"),
            Failed::Network(e) => write!(f, "network: {e}"),
        }
    }
}

impl From<Refused> for Failed {
    fn from(r: Refused) -> Self {
        Failed::Refused(r)
    }
}

/// Fetch `url` as the server does: the system resolver, public addresses
/// only.
pub async fn fetch(url: &str, limits: Limits) -> Result<Image, Failed> {
    let policy = Policy {
        lookup: &system_lookup,
        allowed: &address::public,
        ports: &[80, 443],
    };
    fetch_with(url, limits, &policy).await
}

/// Where a fetch may go: how hosts resolve, which addresses and ports
/// pass. The server's is [`fetch`]'s; tests stand up a server on loopback
/// at some port and let that one address and port through.
pub struct Policy<'a> {
    pub lookup: &'a Lookup,
    pub allowed: &'a (dyn Fn(IpAddr) -> Result<(), Refused> + Sync),
    pub ports: &'a [u16],
}

/// The addresses `host` resolves to.
pub type Lookup = dyn Fn(&str, u16) -> futures_lookup::LookupFuture + Sync;

mod futures_lookup {
    pub type LookupFuture = std::pin::Pin<
        Box<dyn std::future::Future<Output = std::io::Result<Vec<std::net::IpAddr>>> + Send>,
    >;
}

fn system_lookup(host: &str, port: u16) -> futures_lookup::LookupFuture {
    let target = format!("{host}:{port}");
    Box::pin(async move {
        Ok(tokio::net::lookup_host(target)
            .await?
            .map(|a| a.ip())
            .collect())
    })
}

/// [`fetch`] under the given policy.
pub async fn fetch_with(url: &str, limits: Limits, policy: &Policy<'_>) -> Result<Image, Failed> {
    let (lookup, allowed) = (policy.lookup, policy.allowed);
    let mut url = url::Url::parse(url).map_err(|_| Failed::Refused(Refused::Host))?;
    for _ in 0..=limits.max_redirects {
        let target = address::target_with(&url, allowed, policy.ports)?;
        let ip = match target.literal {
            Some(ip) => ip,
            None => {
                let ips = lookup(&target.host, target.port)
                    .await
                    .map_err(|e| Failed::Network(e.to_string()))?;
                // every answer must pass: the client could have used any
                for ip in &ips {
                    allowed(*ip)?;
                }
                *ips.first().ok_or(Failed::Refused(Refused::Host))?
            }
        };
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .resolve(&target.host, SocketAddr::new(ip, target.port))
            .no_proxy()
            .timeout(limits.timeout)
            .user_agent("mailrs-image-fetch")
            .build()
            .map_err(|e| Failed::Network(e.to_string()))?;
        let mut resp = client
            .get(url.clone())
            .send()
            .await
            .map_err(|e| Failed::Network(e.to_string()))?;
        let status = resp.status();
        if status.is_redirection() {
            let next = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or(Failed::Status(status.as_u16()))?;
            url = url.join(next).map_err(|_| Failed::Refused(Refused::Host))?;
            continue;
        }
        if status != reqwest::StatusCode::OK {
            return Err(Failed::Status(status.as_u16()));
        }
        let kind = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !kind.starts_with("image/") {
            return Err(Failed::NotAnImage(kind));
        }
        if resp
            .content_length()
            .is_some_and(|n| n as usize > limits.max_bytes)
        {
            return Err(Failed::TooLarge);
        }
        let mut body = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| Failed::Network(e.to_string()))?
        {
            if body.len() + chunk.len() > limits.max_bytes {
                return Err(Failed::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        return reencode::reencode(&body, limits.max_side).map_err(Failed::Unreadable);
    }
    Err(Failed::TooManyRedirects)
}
