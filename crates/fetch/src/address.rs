//! Which URLs and addresses the fetcher may reach.
//!
//! A URL in a message is chosen by whoever sent it, so every check here
//! assumes it was written to reach something inside: a private range, the
//! cloud metadata address, the docker bridge, or one of these spelled so
//! a naive parser misses it.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Why a URL or an address is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    Scheme,
    Port,
    Credentials,
    Host,
    Address(IpAddr),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::Scheme => write!(f, "only http and https"),
            Refused::Port => write!(f, "only ports 80 and 443"),
            Refused::Credentials => write!(f, "a URL with a user name or password"),
            Refused::Host => write!(f, "no host to fetch from"),
            Refused::Address(ip) => write!(f, "{ip} is not a public address"),
        }
    }
}

/// What may be fetched from a URL: its host and port, the host already a
/// literal address when the URL spelled one in any form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub host: String,
    pub port: u16,
    /// The host when it is an address itself; no lookup is made then.
    pub literal: Option<IpAddr>,
}

/// Check a URL's scheme, port and credentials, and read its host.
pub fn target(url: &url::Url) -> Result<Target, Refused> {
    target_with(url, &public, &[80, 443])
}

/// [`target`] with the address policy and ports given.
pub fn target_with(
    url: &url::Url,
    allowed: &(dyn Fn(IpAddr) -> Result<(), Refused> + Sync),
    ports: &[u16],
) -> Result<Target, Refused> {
    let default = match url.scheme() {
        "http" => 80,
        "https" => 443,
        _ => return Err(Refused::Scheme),
    };
    let port = url.port().unwrap_or(default);
    if !ports.contains(&port) {
        return Err(Refused::Port);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Refused::Credentials);
    }
    // `url` already turns decimal, octal and hex IPv4 hosts into an address
    let (host, literal) = match url.host().ok_or(Refused::Host)? {
        url::Host::Ipv4(a) => (a.to_string(), Some(IpAddr::V4(a))),
        url::Host::Ipv6(a) => (a.to_string(), Some(IpAddr::V6(a))),
        url::Host::Domain(d) => (d.trim_end_matches('.').to_ascii_lowercase(), None),
    };
    if host.is_empty() {
        return Err(Refused::Host);
    }
    if let Some(ip) = literal {
        allowed(ip)?;
    }
    Ok(Target {
        host,
        port,
        literal,
    })
}

/// `Ok` only for an address on the public internet.
pub fn public(ip: IpAddr) -> Result<(), Refused> {
    let open = match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => public_v4(v4),
            None => public_v6(v6),
        },
    };
    match open {
        true => Ok(()),
        false => Err(Refused::Address(ip)),
    }
}

fn public_v4(a: Ipv4Addr) -> bool {
    let [o0, o1, ..] = a.octets();
    !(a.is_unspecified()
        || a.is_loopback()
        || a.is_private()
        || a.is_link_local()
        || a.is_broadcast()
        || a.is_multicast()
        || a.is_documentation()
        || o0 == 0
        // CGNAT, 100.64.0.0/10
        || (o0 == 100 && (64..=127).contains(&o1))
        // IETF protocol assignments, 192.0.0.0/24
        || (o0 == 192 && o1 == 0 && a.octets()[2] == 0)
        // benchmarking, 198.18.0.0/15
        || (o0 == 198 && (18..=19).contains(&o1))
        // reserved, 240.0.0.0/4
        || o0 >= 240)
}

fn public_v6(a: Ipv6Addr) -> bool {
    let s0 = a.segments()[0];
    !(a.is_unspecified()
        || a.is_loopback()
        || a.is_multicast()
        // unique local, fc00::/7
        || (s0 & 0xfe00) == 0xfc00
        // link local, fe80::/10
        || (s0 & 0xffc0) == 0xfe80
        // documentation, 2001:db8::/32
        || (s0 == 0x2001 && a.segments()[1] == 0x0db8)
        // NAT64 well-known prefix, 64:ff9b::/96, which can reach IPv4 inside
        || (s0 == 0x64 && a.segments()[1] == 0xff9b)
        // IPv4-compatible (deprecated) ::a.b.c.d
        || a.segments()[..6].iter().all(|s| *s == 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(url: &str) -> Result<Target, Refused> {
        target(&url::Url::parse(url).unwrap())
    }

    #[test]
    fn inside_addresses_are_refused_however_they_are_written() {
        for url in [
            "http://127.0.0.1/a.png",
            "http://2130706433/a.png", // 127.0.0.1, decimal
            "http://0177.0.0.1/a.png", // octal
            "http://0x7f.0.0.1/a.png", // hex
            "http://10.0.0.5/a.png",
            "http://172.18.0.1/a.png", // a docker bridge gateway
            "http://192.168.1.1/a.png",
            "http://169.254.169.254/latest/meta-data/",
            "http://100.64.0.1/a.png", // CGNAT
            "http://0.0.0.0/a.png",
            "http://[::1]/a.png",
            "http://[fd00::1]/a.png",          // unique local
            "http://[fe80::1]/a.png",          // link local
            "http://[::ffff:127.0.0.1]/a.png", // IPv4-mapped
            "http://[::ffff:a9fe:a9fe]/a.png", // 169.254.169.254, mapped
            "http://[64:ff9b::a00:1]/a.png",   // NAT64 of 10.0.0.1
        ] {
            assert!(
                matches!(check(url), Err(Refused::Address(_))),
                "{url} was let through"
            );
        }
    }

    #[test]
    fn only_web_urls_on_their_own_ports() {
        assert_eq!(check("ftp://example.com/a.png"), Err(Refused::Scheme));
        assert_eq!(check("file:///etc/passwd"), Err(Refused::Scheme));
        assert_eq!(check("http://example.com:8080/a.png"), Err(Refused::Port));
        assert_eq!(
            check("http://u:p@example.com/a.png"),
            Err(Refused::Credentials)
        );
        let ok = check("https://Images.Example.COM./a.png").unwrap();
        assert_eq!(
            (ok.host.as_str(), ok.port, ok.literal),
            ("images.example.com", 443, None)
        );
        assert!(
            check("http://93.184.215.14/a.png").is_ok(),
            "a public address"
        );
    }
}
