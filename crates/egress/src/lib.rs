//! Every request the server makes to an address a user supplied — a
//! calendar subscription, a webhook, a sender's unsubscribe link, a
//! domain's icon — goes through here.
//!
//! - only `http` and `https`, no credentials in the address
//! - every address the name resolves to must be public (no private,
//!   loopback, link-local and cloud metadata, carrier-grade NAT and
//!   tailnet, documentation, benchmark, multicast or reserved ranges;
//!   IPv6 only global unicast, which leaves out every form that embeds
//!   an IPv4 address), and the connection is pinned to those addresses so
//!   a second lookup cannot answer differently
//! - no system proxy
//! - redirects are not followed unless asked, and then each hop is checked
//!   again and may not go from `https` to `http`
//! - bodies are read up to a limit
//!
//! Special ranges: <https://www.iana.org/assignments/iana-ipv4-special-registry/>
//! and <https://www.iana.org/assignments/iana-ipv6-special-registry/>.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use reqwest::header::LOCATION;
use reqwest::{Client, Method, RequestBuilder, Response, Url};

/// Why a request was not made, or not finished.
#[derive(Debug, thiserror::Error)]
pub enum Refused {
    #[error("not an http(s) address")]
    InvalidUrl,
    #[error("not a public address")]
    NonPublicAddress,
    #[error("the name did not resolve")]
    DnsFailed,
    #[error("a redirect from https to http")]
    InsecureRedirect,
    #[error("a redirect without a usable location")]
    InvalidRedirect,
    #[error("too many redirects")]
    TooManyRedirects,
    #[error("the response is larger than {0} bytes")]
    TooLarge(usize),
    #[error("{0}")]
    Http(#[from] reqwest::Error),
}

/// Whether `ip` is an address on the public internet.
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(matches!(a, 0 | 10 | 127)
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192
                    && (b == 168 || (b == 0 && matches!(c, 0 | 2)) || (b == 88 && c == 99)))
                || (a == 198 && (matches!(b, 18 | 19) || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            (s[0] & 0xe000) == 0x2000
                && !(s[0] == 0x2001 && (s[1] < 0x0200 || s[1] == 0x0db8))
                && s[0] != 0x2002
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}

/// `value` as an address this module will request, before any lookup.
pub fn parse(value: &str) -> Result<Url, Refused> {
    if value.len() > 2048 {
        return Err(Refused::InvalidUrl);
    }
    let mut url = Url::parse(value.trim()).map_err(|_| Refused::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(Refused::InvalidUrl);
    }
    url.set_fragment(None);
    let host = url
        .host_str()
        .ok_or(Refused::InvalidUrl)?
        .trim_matches(['[', ']']);
    if let Ok(ip) = host.parse::<IpAddr>() {
        if !public_ip(ip) {
            return Err(Refused::NonPublicAddress);
        }
    } else {
        let name = host.trim_end_matches('.');
        let private_name = !name.contains('.')
            || [
                "localhost",
                ".localhost",
                ".local",
                ".internal",
                ".invalid",
                ".test",
                ".home.arpa",
            ]
            .iter()
            .any(|suffix| name == suffix.trim_start_matches('.') || name.ends_with(suffix));
        if private_name {
            return Err(Refused::NonPublicAddress);
        }
    }
    Ok(url)
}

/// Where a redirect from `from` to `location` goes, if it may be followed.
pub fn next_hop(from: &Url, location: &str) -> Result<Url, Refused> {
    let next = from.join(location).map_err(|_| Refused::InvalidRedirect)?;
    let next = parse(next.as_str())?;
    if from.scheme() == "https" && next.scheme() != "https" {
        return Err(Refused::InsecureRedirect);
    }
    Ok(next)
}

/// A lookup's answer may be connected to only when every address in it is
/// public: a name with one private address among public ones is refused,
/// since which one a connection would take is not ours to choose.
fn all_public(addresses: &[SocketAddr]) -> Result<(), Refused> {
    match !addresses.is_empty()
        && addresses.len() <= 32
        && addresses.iter().all(|a| public_ip(a.ip()))
    {
        true => Ok(()),
        false => Err(Refused::NonPublicAddress),
    }
}

/// Requests to user-supplied addresses, with one user agent and timeout.
#[derive(Debug, Clone)]
pub struct Egress {
    user_agent: &'static str,
    timeout: Duration,
}

impl Egress {
    pub fn new(user_agent: &'static str, timeout: Duration) -> Self {
        Self {
            user_agent,
            timeout,
        }
    }

    /// A client that can reach only the public addresses `url`'s host
    /// resolves to right now, and follows no redirect itself.
    async fn pinned(&self, url: &Url) -> Result<Client, Refused> {
        let host = url
            .host_str()
            .ok_or(Refused::InvalidUrl)?
            .trim_matches(['[', ']']);
        let port = url.port_or_known_default().ok_or(Refused::InvalidUrl)?;
        let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| Refused::DnsFailed)?
            .take(33)
            .collect();
        all_public(&addresses)?;
        Ok(Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .resolve_to_addrs(host, &addresses)
            .connect_timeout(Duration::from_secs(5))
            .timeout(self.timeout)
            .user_agent(self.user_agent)
            .build()?)
    }

    /// Send the request `build` makes to `url`. When the answer is a
    /// redirect and `max_redirects` allows, the next hop is checked and
    /// fetched with a plain GET (what a browser does after a form post);
    /// the first request's headers and body are not carried along.
    pub async fn send(
        &self,
        url: &str,
        max_redirects: u8,
        build: impl FnOnce(&Client, Url) -> RequestBuilder,
    ) -> Result<Response, Refused> {
        let mut url = parse(url)?;
        let mut response = build(&self.pinned(&url).await?, url.clone()).send().await?;
        for _ in 0..max_redirects {
            if !response.status().is_redirection() {
                return Ok(response);
            }
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|h| h.to_str().ok())
                .ok_or(Refused::InvalidRedirect)?;
            url = next_hop(&url, location)?;
            response = self
                .pinned(&url)
                .await?
                .request(Method::GET, url.clone())
                .send()
                .await?;
        }
        if response.status().is_redirection() && max_redirects > 0 {
            return Err(Refused::TooManyRedirects);
        }
        Ok(response)
    }
}

/// The body, refusing one longer than `max` bytes.
pub async fn read_capped(mut response: Response, max: usize) -> Result<Vec<u8>, Refused> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len() + chunk.len() > max {
            return Err(Refused::TooLarge(max));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_addresses_are_never_public() {
        for value in [
            "0.1.2.3",
            "10.1.2.3",
            "100.64.0.1",
            "100.100.100.200",
            "127.0.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "172.31.255.255",
            "192.0.0.9",
            "192.0.2.1",
            "192.88.99.1",
            "192.168.1.1",
            "198.18.1.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::ffff:8.8.8.8",
            "::ffff:127.0.0.1",
            "64:ff9b::808:808",
            "fc00::1",
            "fd7a:115c:a1e0::1",
            "fe80::1",
            "ff02::1",
            "2001::1",
            "2001:20::1",
            "2001:db8::1",
            "2002:0808:0808::1",
            "3fff::1",
            "5f00::1",
        ] {
            assert_eq!(value.parse().map(public_ip), Ok(false), "{value}");
        }
        for value in [
            "8.8.8.8",
            "1.1.1.1",
            "52.195.89.111",
            "2001:4860:4860::8888",
            "2606:4700:4700::1111",
        ] {
            assert_eq!(value.parse().map(public_ip), Ok(true), "{value}");
        }
    }

    #[test]
    fn addresses_that_hide_a_private_destination_are_refused() {
        for value in [
            "file:///etc/passwd",
            "gopher://example.com/",
            "http://2130706433",
            "http://0x7f000001",
            "http://0177.0.0.1",
            "http://127.1",
            "http://[::ffff:127.0.0.1]",
            "http://[::1]:8080/",
            "http://169.254.169.254/latest/meta-data/",
            "http://100.100.100.200/",
            "http://user:pass@example.com",
            "http://localhost:3201/",
            "http://localhost./",
            "http://kevy-server:6379/",
            "http://metadata.google.internal",
            "http://printer.local/",
            "http://router.home.arpa/",
        ] {
            assert!(parse(value).is_err(), "{value}");
        }
        let ok = parse("https://example.com/feed.ics?x=1#frag").unwrap();
        assert_eq!(ok.as_str(), "https://example.com/feed.ics?x=1");
        assert!(parse("https://example.com:8443/hook").is_ok());
    }

    #[test]
    fn redirects_are_checked_like_the_first_address() {
        let from = parse("https://example.com/a").unwrap();
        assert_eq!(
            next_hop(&from, "/b").unwrap().as_str(),
            "https://example.com/b"
        );
        assert!(matches!(
            next_hop(&from, "http://example.com/b"),
            Err(Refused::InsecureRedirect)
        ));
        assert!(matches!(
            next_hop(&from, "http://127.0.0.1/"),
            Err(Refused::NonPublicAddress)
        ));
        assert!(matches!(
            next_hop(&from, "https://169.254.169.254/"),
            Err(Refused::NonPublicAddress)
        ));
        let plain = parse("http://example.com/a").unwrap();
        assert!(next_hop(&plain, "https://example.org/").is_ok());
    }

    #[test]
    fn a_lookup_with_any_private_answer_is_refused() {
        let at = |ip: &str| SocketAddr::new(ip.parse().unwrap(), 443);
        assert!(all_public(&[at("93.184.216.34")]).is_ok());
        assert!(all_public(&[at("93.184.216.34"), at("127.0.0.1")]).is_err());
        assert!(all_public(&[at("10.0.0.5")]).is_err());
        assert!(all_public(&[]).is_err());
    }
}
