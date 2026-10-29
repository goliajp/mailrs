//! In-process storage, for a single process and for tests.

use std::collections::HashMap;
use std::io;
use std::net::IpAddr;
use std::sync::Mutex;

use async_trait::async_trait;

use super::{
    LoginGuard, LoginPolicy, Ops, Verdict, check, record_failure, record_success, unix_now,
};
use crate::events::{LoginEvent, Query};

enum Value {
    Counter(u64),
    Hash(HashMap<String, u64>),
    List(Vec<Vec<u8>>),
    Set(std::collections::HashSet<String>),
}

/// In-process storage for [`MemoryLoginGuard`] and for tests.
#[derive(Default)]
pub struct MemoryOps {
    entries: HashMap<String, (Value, u64)>,
    now: u64,
}

impl MemoryOps {
    /// Set the clock the expiries are read against.
    pub fn at(&mut self, now: u64) -> &mut Self {
        self.now = now;
        self.entries.retain(|_, (_, until)| *until > now);
        self
    }

    fn counter(&self, key: &str) -> u64 {
        match self.entries.get(key) {
            Some((Value::Counter(n), _)) => *n,
            _ => 0,
        }
    }
}

impl Ops for MemoryOps {
    fn get(&mut self, key: &str) -> io::Result<Option<u64>> {
        Ok(match self.entries.get(key) {
            Some((Value::Counter(n), _)) => Some(*n),
            _ => None,
        })
    }

    fn set(&mut self, key: &str, value: u64, ttl_secs: u64) -> io::Result<()> {
        self.entries.insert(
            key.to_string(),
            (Value::Counter(value), self.now + ttl_secs),
        );
        Ok(())
    }

    fn incr(&mut self, key: &str, ttl_secs: u64) -> io::Result<u64> {
        let now = self.now;
        let entry = self
            .entries
            .entry(key.to_string())
            .or_insert((Value::Counter(0), now + ttl_secs));
        let Value::Counter(n) = &mut entry.0 else {
            return Err(io::Error::other("not a counter"));
        };
        *n += 1;
        Ok(*n)
    }

    fn counts(&mut self, keys: &[String]) -> io::Result<Vec<u64>> {
        Ok(keys.iter().map(|k| self.counter(k)).collect())
    }

    fn del(&mut self, keys: &[String]) -> io::Result<()> {
        for k in keys {
            self.entries.remove(k);
        }
        Ok(())
    }

    fn hget(&mut self, key: &str, field: &str) -> io::Result<Option<u64>> {
        Ok(match self.entries.get(key) {
            Some((Value::Hash(h), _)) => h.get(field).copied(),
            _ => None,
        })
    }

    fn hgetall(&mut self, key: &str) -> io::Result<Vec<(String, u64)>> {
        Ok(match self.entries.get(key) {
            Some((Value::Hash(h), _)) => h.iter().map(|(f, v)| (f.clone(), *v)).collect(),
            _ => Vec::new(),
        })
    }

    fn hset(&mut self, key: &str, field: &str, value: u64, ttl_secs: u64) -> io::Result<()> {
        let until = self.now + ttl_secs;
        let entry = self
            .entries
            .entry(key.to_string())
            .or_insert_with(|| (Value::Hash(HashMap::new()), until));
        entry.1 = until;
        let Value::Hash(h) = &mut entry.0 else {
            return Err(io::Error::other("not a hash"));
        };
        h.insert(field.to_string(), value);
        Ok(())
    }

    fn hdel(&mut self, key: &str, fields: &[String]) -> io::Result<()> {
        if let Some((Value::Hash(h), _)) = self.entries.get_mut(key) {
            for f in fields {
                h.remove(f);
            }
        }
        Ok(())
    }
}

/// The guard for a single process (and tests): state lives in memory.
#[derive(Default)]
pub struct MemoryLoginGuard {
    policy: LoginPolicy,
    ops: Mutex<MemoryOps>,
}

impl MemoryLoginGuard {
    /// A guard with `policy`.
    pub fn new(policy: LoginPolicy) -> Self {
        Self {
            policy,
            ops: Mutex::new(MemoryOps::default()),
        }
    }

    fn run<R>(
        &self,
        f: impl FnOnce(&mut MemoryOps, &LoginPolicy) -> io::Result<R>,
    ) -> io::Result<R> {
        let mut ops = self.ops.lock().unwrap_or_else(|e| e.into_inner());
        f(ops.at(unix_now()), &self.policy)
    }
}

#[async_trait]
impl LoginGuard for MemoryLoginGuard {
    async fn check(&self, source: IpAddr, account: &str) -> Verdict {
        self.run(|ops, p| {
            let now = ops.now;
            check(ops, p, source, account, now)
        })
        .unwrap_or(Verdict::Allowed)
    }

    async fn failed(&self, source: IpAddr, account: &str) {
        let _ = self.run(|ops, p| {
            let now = ops.now;
            record_failure(ops, p, source, account, now)
        });
    }

    async fn succeeded(&self, source: IpAddr, account: &str) {
        let _ = self.run(|ops, p| {
            let now = ops.now;
            record_success(ops, p, source, account, now)
        });
    }

    async fn record(&self, event: LoginEvent) {
        if !matches!(
            self.run(|ops, _| crate::events::store(ops, &event)),
            Ok(false)
        ) {
            event.log();
        }
    }

    async fn raise(&self, alert: crate::alerts::Alert) -> io::Result<bool> {
        self.run(|ops, _| crate::alerts::raise(ops, &alert))
    }

    async fn take_alerts(&self, max: usize) -> io::Result<Vec<crate::alerts::Alert>> {
        self.run(|ops, _| crate::alerts::take(ops, max))
    }

    async fn events(&self, query: Query) -> io::Result<Vec<LoginEvent>> {
        self.run(|ops, _| {
            let now = ops.now;
            crate::events::query(ops, &query, now)
        })
    }
}

impl crate::events::EventOps for MemoryOps {
    fn len(&mut self, key: &str) -> io::Result<u64> {
        Ok(match self.entries.get(key) {
            Some((Value::List(l), _)) => l.len() as u64,
            _ => 0,
        })
    }

    fn push(&mut self, key: &str, value: &[u8], ttl_secs: u64) -> io::Result<()> {
        let until = self.now + ttl_secs;
        let entry = self
            .entries
            .entry(key.to_string())
            .or_insert_with(|| (Value::List(Vec::new()), until));
        let Value::List(l) = &mut entry.0 else {
            return Err(io::Error::other("not a list"));
        };
        l.insert(0, value.to_vec());
        Ok(())
    }

    fn range(&mut self, key: &str, start: u64, stop: u64) -> io::Result<Vec<Vec<u8>>> {
        Ok(match self.entries.get(key) {
            Some((Value::List(l), _)) => l
                .iter()
                .skip(start as usize)
                .take((stop.saturating_sub(start) + 1) as usize)
                .cloned()
                .collect(),
            _ => Vec::new(),
        })
    }

    fn add_member(&mut self, key: &str, member: &str, ttl_secs: u64) -> io::Result<bool> {
        let until = self.now + ttl_secs;
        let entry = self
            .entries
            .entry(key.to_string())
            .or_insert_with(|| (Value::Set(Default::default()), until));
        let Value::Set(set) = &mut entry.0 else {
            return Err(io::Error::other("not a set"));
        };
        Ok(set.insert(member.to_string()))
    }

    fn bump(&mut self, key: &str, ttl_secs: u64) -> io::Result<()> {
        Ops::incr(self, key, ttl_secs).map(|_| ())
    }

    fn pop_oldest(&mut self, key: &str) -> io::Result<Option<Vec<u8>>> {
        Ok(match self.entries.get_mut(key) {
            Some((Value::List(l), _)) => l.pop(),
            _ => None,
        })
    }
}
