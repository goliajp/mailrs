//! Which server answers `MAILRS_KEVY_URL`, and whether it has every command
//! the side state uses. kevy-server does; Redis from 7.4 and Valkey from 9.0,
//! the first with per-field hash expiry (`HEXPIRE`), which push devices,
//! submissions and test mailboxes rely on. An older server takes most
//! commands and refuses those, so the processes refuse it at boot instead.

/// The server, by name and version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Server {
    Kevy(String),
    Valkey(String),
    Redis(String),
}

impl std::fmt::Display for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Server::Kevy(v) => write!(f, "kevy-server {v}"),
            Server::Valkey(v) => write!(f, "Valkey {v}"),
            Server::Redis(v) => write!(f, "Redis {v}"),
        }
    }
}

/// The server an `INFO server` reply describes, if it is new enough. Valkey
/// reports a `redis_version` too (the Redis release it is compatible
/// with), and kevy-server a `redis_version` and its own, so the specific
/// name is read first.
pub fn supported(info: &str) -> Result<Server, String> {
    let field = |name: &str| {
        info.lines()
            .find_map(|l| l.strip_prefix(name)?.strip_prefix(':'))
            .map(|v| v.trim().to_string())
    };
    if let Some(v) = field("kevy_version") {
        return Ok(Server::Kevy(v));
    }
    if let Some(v) = field("valkey_version") {
        return at_least(&v, (9, 0))
            .then(|| Server::Valkey(v.clone()))
            .ok_or(format!(
                "Valkey {v} has no per-field hash expiry; 9.0 or later is needed"
            ));
    }
    match field("redis_version") {
        Some(v) => at_least(&v, (7, 4))
            .then(|| Server::Redis(v.clone()))
            .ok_or(format!(
                "Redis {v} has no per-field hash expiry; 7.4 or later is needed"
            )),
        None => Err("the server does not say what it is (no version in INFO server)".into()),
    }
}

fn at_least(version: &str, min: (u32, u32)) -> bool {
    let mut parts = version.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let have = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    have >= min
}

/// Check the server at `url` at boot: a server too old to serve the side
/// state stops the process with the reason; one that cannot be reached is
/// left to the process, which reconnects as it does whenever the network
/// kevy is down.
pub fn require_supported(url: &str) {
    let info = kevy_client::Connection::connect(url).and_then(|mut c| {
        c.pipeline(|p| {
            p.cmd(&[b"INFO", b"server"]);
        })
    });
    let text = match info.as_deref() {
        Ok([kevy_client::Reply::Bulk(b)]) => String::from_utf8_lossy(b).into_owned(),
        Ok(_) => String::new(),
        Err(e) => {
            tracing::warn!(error = %e, "network store: not reachable at boot; version not checked");
            return;
        }
    };
    match supported(&text) {
        Ok(server) => tracing::info!(%server, "network store"),
        Err(why) => {
            tracing::error!(%why, "network store: refusing to start");
            eprintln!("MAILRS_KEVY_URL: {why}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_server_is_known_by_its_own_name() {
        let kevy = "# Server\r\nredis_version:7.4.0\r\nkevy_version:6.4.0\r\n";
        let valkey = "# Server\r\nredis_version:7.2.4\r\nvalkey_version:9.0.7\r\n";
        let redis = "# Server\r\nredis_version:7.4.12\r\n";
        assert_eq!(supported(kevy), Ok(Server::Kevy("6.4.0".into())));
        assert_eq!(supported(valkey), Ok(Server::Valkey("9.0.7".into())));
        assert_eq!(supported(redis), Ok(Server::Redis("7.4.12".into())));
    }

    #[test]
    fn a_server_without_hash_field_expiry_is_refused() {
        assert!(supported("redis_version:7.2.5\r\n").is_err());
        assert!(supported("redis_version:6.2.14\r\n").is_err());
        assert!(supported("redis_version:7.2.4\r\nvalkey_version:8.1.1\r\n").is_err());
        assert!(supported("redis_version:8.0.0\r\n").is_ok());
        assert!(supported("# Server\r\n").is_err());
    }
}
