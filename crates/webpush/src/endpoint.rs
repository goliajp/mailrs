//! Which push endpoints a subscription may name. The endpoint comes from
//! the browser, so it is checked twice: the host must be a known browser
//! push service, and every address it resolves to must be public — a
//! service's name pointed at a private address is refused too.
//!
//! A UnifiedPush endpoint comes from whatever distributor the reader's
//! phone uses, so no list of services can name them: the host only has to
//! be a name, over https on 443, and the address check carries the weight.

use std::net::{IpAddr, SocketAddr};

use mailrs_fetch::address::{Refused, public};

/// The browser push services, by host or by parent domain (`.` first).
const SERVICES: [&str; 4] = [
    // Chrome, Edge, Opera, Samsung Internet
    "fcm.googleapis.com",
    // Firefox
    "updates.push.services.mozilla.com",
    // Safari
    "web.push.apple.com",
    // Windows notification service, for older Edge
    ".notify.windows.com",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejected {
    NotHttps,
    Port,
    /// Not one of the known push services.
    Host(String),
    /// An address where a name belongs.
    NotAName(String),
    /// The service's name resolved to an address that is not public.
    Address(IpAddr),
    /// The name did not resolve at all.
    Unresolved,
}

impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Rejected::NotHttps => write!(f, "a push endpoint must be https"),
            Rejected::Port => write!(f, "a push endpoint must be on port 443"),
            Rejected::Host(h) => write!(f, "{h} is not a known browser push service"),
            Rejected::NotAName(h) => write!(f, "{h} is not a host name"),
            Rejected::Address(ip) => write!(f, "{ip} is not a public address"),
            Rejected::Unresolved => write!(f, "the push service's name does not resolve"),
        }
    }
}

/// The endpoint's host name, when it is https on 443 and names a host.
fn https_name(endpoint: &url::Url) -> Result<String, Rejected> {
    if endpoint.scheme() != "https" {
        return Err(Rejected::NotHttps);
    }
    if endpoint.port_or_known_default() != Some(443) {
        return Err(Rejected::Port);
    }
    let Some(url::Host::Domain(host)) = endpoint.host() else {
        return Err(Rejected::NotAName(
            endpoint.host_str().unwrap_or_default().to_string(),
        ));
    };
    Ok(host.trim_end_matches('.').to_ascii_lowercase())
}

/// A UnifiedPush endpoint's host. Its addresses are checked when it is
/// resolved, by [`resolve_distributor`].
pub fn distributor_host(endpoint: &url::Url) -> Result<String, Rejected> {
    https_name(endpoint)
}

/// The endpoint's host, when the URL may be pushed to at all.
pub fn host(endpoint: &url::Url) -> Result<String, Rejected> {
    let host = https_name(endpoint)?;
    let known = SERVICES.iter().any(|s| match s.strip_prefix('.') {
        Some(parent) => host.ends_with(s) && host.len() > parent.len() + 1,
        None => host == *s,
    });
    match known {
        true => Ok(host),
        false => Err(Rejected::Host(host)),
    }
}

/// The address to connect to: the first the name resolves to, provided
/// every one of them is public (a name that sometimes answers privately is
/// refused, so the check and the connection cannot be split).
pub fn checked_address(addrs: &[IpAddr]) -> Result<SocketAddr, Rejected> {
    let first = addrs.first().ok_or(Rejected::Unresolved)?;
    for ip in addrs {
        public(*ip).map_err(|e| match e {
            Refused::Address(ip) => Rejected::Address(ip),
            _ => Rejected::Address(*ip),
        })?;
    }
    Ok(SocketAddr::new(*first, 443))
}

/// Resolve the endpoint's host and check it; the host and the address to
/// connect to.
pub async fn resolve(endpoint: &url::Url) -> Result<(String, SocketAddr), Rejected> {
    lookup(host(endpoint)?).await
}

/// [`resolve`] for a UnifiedPush endpoint: any host name, every address
/// public.
pub async fn resolve_distributor(endpoint: &url::Url) -> Result<(String, SocketAddr), Rejected> {
    lookup(distributor_host(endpoint)?).await
}

async fn lookup(host: String) -> Result<(String, SocketAddr), Rejected> {
    let addrs: Vec<IpAddr> = tokio::net::lookup_host((host.as_str(), 443))
        .await
        .map_err(|_| Rejected::Unresolved)?
        .map(|a| a.ip())
        .collect();
    Ok((host, checked_address(&addrs)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(url: &str) -> Result<String, Rejected> {
        host(&url::Url::parse(url).unwrap())
    }

    #[test]
    fn only_the_browser_push_services() {
        assert_eq!(
            h("https://fcm.googleapis.com/fcm/send/x").unwrap(),
            "fcm.googleapis.com"
        );
        assert!(h("https://updates.push.services.mozilla.com/wpush/v2/x").is_ok());
        assert!(h("https://web.push.apple.com/QGuQ").is_ok());
        assert!(h("https://wns2-by3p.notify.windows.com/w/?token=x").is_ok());
        assert_eq!(
            h("https://fcm.googleapis.com.evil.test/x"),
            Err(Rejected::Host("fcm.googleapis.com.evil.test".into()))
        );
        assert!(matches!(
            h("https://notify.windows.com/x"),
            Err(Rejected::Host(_))
        ));
        assert!(matches!(
            h("https://evilnotify.windows.com/x"),
            Err(Rejected::Host(_))
        ));
        assert_eq!(h("http://fcm.googleapis.com/x"), Err(Rejected::NotHttps));
        assert_eq!(h("https://fcm.googleapis.com:8443/x"), Err(Rejected::Port));
        assert!(matches!(
            h("https://10.0.0.1/x"),
            Err(Rejected::NotAName(_))
        ));
    }

    #[test]
    fn a_distributor_is_any_https_name_on_443() {
        let d = |url: &str| distributor_host(&url::Url::parse(url).unwrap());
        assert_eq!(
            d("https://ntfy.example.org/upAbC123?up=1").unwrap(),
            "ntfy.example.org"
        );
        assert_eq!(d("http://ntfy.example.org/up1"), Err(Rejected::NotHttps));
        assert_eq!(d("https://ntfy.example.org:8080/up1"), Err(Rejected::Port));
        assert_eq!(
            d("https://192.168.1.10/up1"),
            Err(Rejected::NotAName("192.168.1.10".into()))
        );
        assert!(matches!(d("https://[::1]/up1"), Err(Rejected::NotAName(_))));
    }

    #[tokio::test]
    async fn a_distributor_name_that_resolves_to_loopback_is_refused() {
        let url = url::Url::parse("https://localhost/up1").unwrap();
        assert!(matches!(
            resolve_distributor(&url).await,
            Err(Rejected::Address(ip)) if ip.is_loopback()
        ));
    }

    #[test]
    fn a_service_name_that_resolves_privately_is_refused() {
        let public_ip: IpAddr = "142.250.196.10".parse().unwrap();
        let private: IpAddr = "10.1.2.3".parse().unwrap();
        let metadata: IpAddr = "169.254.169.254".parse().unwrap();
        assert_eq!(
            checked_address(&[public_ip]).unwrap(),
            SocketAddr::new(public_ip, 443)
        );
        assert_eq!(
            checked_address(&[public_ip, private]),
            Err(Rejected::Address(private))
        );
        assert_eq!(
            checked_address(&[metadata]),
            Err(Rejected::Address(metadata))
        );
        assert_eq!(checked_address(&[]), Err(Rejected::Unresolved));
    }
}
