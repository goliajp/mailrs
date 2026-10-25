//! The login guard over a shared kevy server, so every process that
//! accepts logins sees the same counts and locks.

use std::io;
use std::net::IpAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use kevy_client::Connection;

use crate::login::{
    LoginGuard, LoginPolicy, Ops, Verdict, check, record_failure, record_success,
    store_unavailable, unix_now,
};

fn bytes(keys: &[String]) -> Vec<&[u8]> {
    keys.iter().map(|k| k.as_bytes()).collect()
}

fn number(raw: &[u8]) -> u64 {
    std::str::from_utf8(raw)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

impl Ops for Connection {
    fn get(&mut self, key: &str) -> io::Result<Option<u64>> {
        let v = Connection::get(self, key.as_bytes()).map_err(io::Error::from)?;
        Ok(v.as_deref().map(number))
    }

    fn set(&mut self, key: &str, value: u64, ttl_secs: u64) -> io::Result<()> {
        self.set_with_ttl(
            key.as_bytes(),
            value.to_string().as_bytes(),
            Duration::from_secs(ttl_secs),
        )
        .map_err(io::Error::from)
    }

    fn incr(&mut self, key: &str, ttl_secs: u64) -> io::Result<u64> {
        let n = self.incr(key.as_bytes()).map_err(io::Error::from)?;
        if n == 1 {
            self.expire(key.as_bytes(), Duration::from_secs(ttl_secs))
                .map_err(io::Error::from)?;
        }
        Ok(u64::try_from(n).unwrap_or(0))
    }

    fn counts(&mut self, keys: &[String]) -> io::Result<Vec<u64>> {
        let values = self.mget(&bytes(keys)).map_err(io::Error::from)?;
        Ok(values
            .iter()
            .map(|v| v.as_deref().map(number).unwrap_or(0))
            .collect())
    }

    fn del(&mut self, keys: &[String]) -> io::Result<()> {
        self.del(&bytes(keys)).map_err(io::Error::from)?;
        Ok(())
    }

    fn hget(&mut self, key: &str, field: &str) -> io::Result<Option<u64>> {
        let v = self
            .hget(key.as_bytes(), field.as_bytes())
            .map_err(io::Error::from)?;
        Ok(v.as_deref().map(number))
    }

    fn hgetall(&mut self, key: &str) -> io::Result<Vec<(String, u64)>> {
        let flat = self.hgetall(key.as_bytes()).map_err(io::Error::from)?;
        Ok(flat
            .chunks_exact(2)
            .map(|pair| {
                (
                    String::from_utf8_lossy(&pair[0]).into_owned(),
                    number(&pair[1]),
                )
            })
            .collect())
    }

    fn hset(&mut self, key: &str, field: &str, value: u64, ttl_secs: u64) -> io::Result<()> {
        let value = value.to_string();
        self.hset(key.as_bytes(), &[(field.as_bytes(), value.as_bytes())])
            .map_err(io::Error::from)?;
        self.expire(key.as_bytes(), Duration::from_secs(ttl_secs))
            .map_err(io::Error::from)?;
        Ok(())
    }

    fn hdel(&mut self, key: &str, fields: &[String]) -> io::Result<()> {
        self.hdel(key.as_bytes(), &bytes(fields))
            .map_err(io::Error::from)?;
        Ok(())
    }
}

impl crate::events::EventOps for Connection {
    fn len(&mut self, key: &str) -> io::Result<u64> {
        Ok(self.llen(key.as_bytes()).map_err(io::Error::from)? as u64)
    }

    fn push(&mut self, key: &str, value: &[u8], ttl_secs: u64) -> io::Result<()> {
        let len = self
            .lpush(key.as_bytes(), &[value])
            .map_err(io::Error::from)?;
        if len == 1 {
            self.expire(key.as_bytes(), Duration::from_secs(ttl_secs))
                .map_err(io::Error::from)?;
        }
        Ok(())
    }

    fn range(&mut self, key: &str, start: u64, stop: u64) -> io::Result<Vec<Vec<u8>>> {
        self.lrange(key.as_bytes(), start as i64, stop as i64)
            .map_err(io::Error::from)
    }

    fn add_member(&mut self, key: &str, member: &str, ttl_secs: u64) -> io::Result<bool> {
        let added = self
            .sadd(key.as_bytes(), &[member.as_bytes()])
            .map_err(io::Error::from)?;
        if added == 1 && self.scard(key.as_bytes()).map_err(io::Error::from)? == 1 {
            self.expire(key.as_bytes(), Duration::from_secs(ttl_secs))
                .map_err(io::Error::from)?;
        }
        Ok(added == 1)
    }

    fn bump(&mut self, key: &str, ttl_secs: u64) -> io::Result<()> {
        Ops::incr(self, key, ttl_secs).map(|_| ())
    }
}

const POOL_SIZE: usize = 4;

/// The guard every process shares through one kevy server. Connections
/// open lazily and a failed one is reopened on next use, so a server that
/// restarts costs the logins of that moment only their lockout, never
/// their success.
pub struct KevyLoginGuard {
    url: String,
    policy: LoginPolicy,
    pool: Vec<Mutex<Option<Connection>>>,
    next: AtomicUsize,
}

impl KevyLoginGuard {
    /// A guard over the server at `url` (`kevy://host:port`).
    pub fn new(url: impl Into<String>, policy: LoginPolicy) -> Arc<Self> {
        Arc::new(Self {
            url: url.into(),
            policy,
            pool: (0..POOL_SIZE).map(|_| Mutex::new(None)).collect(),
            next: AtomicUsize::new(0),
        })
    }

    fn with_ops<R>(
        &self,
        f: impl FnOnce(&mut Connection, &LoginPolicy, u64) -> io::Result<R>,
    ) -> io::Result<R> {
        let idx = self.next.fetch_add(1, Ordering::Relaxed) % self.pool.len();
        let mut slot = self.pool[idx].lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_none() {
            *slot = Some(Connection::connect(&self.url).map_err(io::Error::from)?);
        }
        let conn = slot.as_mut().expect("just opened");
        let out = f(conn, &self.policy, unix_now());
        if out.is_err() {
            *slot = None;
        }
        out
    }

    async fn run<R: Send + 'static>(
        self: &Arc<Self>,
        op: &'static str,
        f: impl FnOnce(&mut Connection, &LoginPolicy, u64) -> io::Result<R> + Send + 'static,
    ) -> Option<R> {
        let me = self.clone();
        let out = tokio::task::spawn_blocking(move || me.with_ops(f))
            .await
            .unwrap_or_else(|e| Err(io::Error::other(e)));
        match out {
            Ok(r) => Some(r),
            Err(e) => {
                store_unavailable(op, &e);
                None
            }
        }
    }
}

/// `Arc<KevyLoginGuard>` is what callers hold, behind `dyn LoginGuard`.
#[async_trait]
impl LoginGuard for Arc<KevyLoginGuard> {
    async fn check(&self, source: IpAddr, account: &str) -> Verdict {
        let account = account.to_string();
        self.run("check", move |o, p, now| check(o, p, source, &account, now))
            .await
            .unwrap_or(Verdict::Allowed)
    }

    async fn failed(&self, source: IpAddr, account: &str) {
        let account = account.to_string();
        self.run("failed", move |o, p, now| {
            record_failure(o, p, source, &account, now)
        })
        .await;
    }

    async fn succeeded(&self, source: IpAddr, account: &str) {
        let account = account.to_string();
        self.run("succeeded", move |o, p, now| {
            record_success(o, p, source, &account, now)
        })
        .await;
    }
    async fn record(&self, event: crate::events::LoginEvent) {
        let line = event.clone();
        let stored = self
            .run("record", move |o, _, _| crate::events::store(o, &event))
            .await;
        if stored != Some(false) {
            line.log();
        }
    }

    async fn events(
        &self,
        query: crate::events::Query,
    ) -> io::Result<Vec<crate::events::LoginEvent>> {
        let me = self.clone();
        tokio::task::spawn_blocking(move || {
            me.with_ops(|o, _, now| crate::events::query(o, &query, now))
        })
        .await
        .map_err(io::Error::other)?
    }
}

/// The guard for a process: over the kevy server at `url` when there is
/// one, so every process counts together; in memory otherwise.
pub fn guard_for(url: Option<&str>) -> Arc<dyn LoginGuard> {
    match url.filter(|u| !u.is_empty()) {
        Some(url) => Arc::new(KevyLoginGuard::new(url, LoginPolicy::default())),
        None => {
            tracing::warn!("no shared kevy: login failures are counted in this process only");
            Arc::new(crate::login::MemoryLoginGuard::default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `mem://` dispatches to an embedded store behind the same command
    // surface the network path uses
    #[test]
    fn kevy_ops_meet_the_contract() {
        let mut n = 0;
        crate::login::contract::check_ops(
            || {
                n += 1;
                Connection::connect(&format!("mem://login-guard-contract-{n}")).expect("mem")
            },
            |_, _| {},
        );
    }

    #[test]
    fn events_are_stored_and_read_back_over_kevy() {
        use crate::events::{Channel, Credential, LoginEvent, Outcome, Query, query, store};
        let mut c = Connection::connect("mem://login-events").expect("mem");
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        let ok = LoginEvent::new(
            Channel::ApiKey,
            Credential::ApiKey,
            ip,
            "a@x.test",
            Outcome::Ok,
        )
        .with_key("mk_06fa1");
        let bad = LoginEvent::new(
            Channel::Imap,
            Credential::Password,
            ip,
            "a@x.test",
            Outcome::BadPassword,
        );
        for e in [&ok, &ok, &bad] {
            store(&mut c, e).unwrap();
        }
        let got = query(&mut c, &Query::default(), unix_now()).unwrap();
        assert_eq!(
            got,
            vec![bad.clone(), ok.clone()],
            "the repeated key success is kept once"
        );
        let q = Query {
            channel: Some(Channel::Imap),
            ..Default::default()
        };
        assert_eq!(query(&mut c, &q, unix_now()).unwrap(), vec![bad]);
    }

    #[tokio::test]
    async fn an_unreachable_server_lets_logins_through() {
        let g: Arc<dyn LoginGuard> = Arc::new(KevyLoginGuard::new(
            "kevy://127.0.0.1:1",
            LoginPolicy::default(),
        ));
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        for _ in 0..6 {
            g.failed(ip, "a@x.test").await;
        }
        assert_eq!(g.check(ip, "a@x.test").await, Verdict::Allowed);
    }
}
