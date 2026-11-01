//! The fact ledger: what each delivered message was, kept so that a change
//! of reputation, a correction or a report can find the mail it concerns
//! and judge only that again.
//!
//! Facts and verdicts are kept apart. A message's facts (`L:m:`, with one
//! row per link host in `L:l:` and per attachment in `L:f:`) are written
//! once at delivery and never edited; its verdict (`L:v:`) is derived from
//! them and the rules, and may be deleted and recomputed wholesale. The
//! query axes — sender domain and address, link host, attachment hash —
//! are declared indexes on the three tables, not lists kept by hand.

use std::io;

use kevy_embedded::{IndexKind, IndexValType as ValType, TableIndex, TableSpec};

use crate::KevyMailboxStore;

mod lookup;

pub(crate) const MSG_PREFIX: &[u8] = b"L:m:";
pub(crate) const LINK_PREFIX: &[u8] = b"L:l:";
pub(crate) const FILE_PREFIX: &[u8] = b"L:f:";

/// What one delivered message was, as delivery read it.
#[derive(Debug, Clone, Default)]
pub struct LedgerFacts {
    pub message_id: String,
    pub user: String,
    pub from_addr: String,
    pub from_domain: String,
    pub reply_to: String,
    pub link_hosts: Vec<String>,
    pub attachment_sha256: Vec<String>,
    /// SPF, DKIM and DMARC results as the receiver recorded them.
    pub auth: String,
    /// Epoch seconds the message arrived.
    pub arrived: i64,
    /// The registrable domain of `from_domain`, which the row is indexed
    /// by, so an observation on a domain finds mail from its subdomains.
    pub from_reg: String,
}

/// A link host to index the message by, with its registrable domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedLink {
    pub host: String,
    pub reg: String,
}

/// What the rules made of a message: derived, recomputed at will.
#[derive(Debug, Clone, Default)]
pub struct LedgerVerdict {
    pub rules: Vec<String>,
    pub score: f64,
    pub rules_version: String,
    pub judged_at: i64,
    /// The fraud rules hold the conversation.
    pub holds: bool,
    /// What the screening rules matched, in whichever mode they run.
    pub screening: Vec<String>,
}

fn col(name: &str, t: ValType) -> (Vec<u8>, ValType) {
    (name.as_bytes().to_vec(), t)
}

fn table(name: &str, prefix: &[u8], columns: Vec<(Vec<u8>, ValType)>, keyed: &[&str]) -> TableSpec {
    let bytes = |n: &str| n.as_bytes().to_vec();
    TableSpec {
        name: bytes(name),
        prefix: prefix.to_vec(),
        pk: bytes("id"),
        window: None,
        autodeclare: 0,
        auto_added: Vec::new(),
        columns,
        indexes: keyed
            .iter()
            .map(|k| TableIndex {
                column: bytes(k),
                kind: IndexKind::Range,
                // nothing beside the key: a lookup reads the row anyway
                values: Vec::new(),
            })
            .collect(),
        orderpaths: Vec::new(),
    }
}

/// The three ledger tables, as declared. A link or file row holds only its
/// id (the message row's id, `|`, the host or hash) and the value it is
/// keyed on; who and when are on the message row it points to.
pub(crate) fn specs() -> [TableSpec; 3] {
    [
        table(
            "ledger_msg",
            MSG_PREFIX,
            vec![
                col("id", ValType::Str),
                col("mid", ValType::Str),
                col("user", ValType::Str),
                col("arrived", ValType::I64),
                col("from_domain", ValType::Str),
                col("from_addr", ValType::Str),
                col("reg", ValType::Str),
            ],
            &["from_domain", "from_addr", "reg"],
        ),
        table(
            "ledger_link",
            LINK_PREFIX,
            vec![
                col("id", ValType::Str),
                col("host", ValType::Str),
                col("until", ValType::I64),
                col("reg", ValType::Str),
            ],
            &["host", "until", "reg"],
        ),
        table(
            "ledger_file",
            FILE_PREFIX,
            vec![col("id", ValType::Str), col("sha", ValType::Str)],
            &["sha"],
        ),
    ]
}

// Keyed by reader and message: the same message delivered to two people is
// two records, because what it means — the sender's history with each of
// them, a hold — is each reader's own.
fn msg_key(user: &str, mid: &str) -> String {
    format!("L:m:{user}|{mid}")
}
fn link_key(user: &str, mid: &str, host: &str) -> String {
    format!("L:l:{user}|{mid}|{host}")
}
fn file_key(user: &str, mid: &str, sha: &str) -> String {
    format!("L:f:{user}|{mid}|{sha}")
}
fn verdict_key(user: &str, mid: &str) -> String {
    format!("L:v:{user}|{mid}")
}

impl KevyMailboxStore {
    /// Declares the ledger tables, or leaves them when already declared
    /// with this shape. A changed shape is dropped and declared again; the
    /// rows stay and the indexes are rebuilt from them.
    pub fn ensure_ledger_tables(&self) -> io::Result<()> {
        let current = self.store().table_list();
        for spec in specs() {
            match current.iter().find(|t| t.name == spec.name) {
                Some(existing) if existing.sans_auto() == spec => continue,
                Some(_) => {
                    self.store().table_drop(&spec.name);
                }
                None => {}
            }
            self.store().table_declare(spec).map_err(io::Error::other)?;
        }
        Ok(())
    }

    /// Records what a delivered message was, for its reader, with a link row
    /// for every host and none of them expiring. Written once: a second call
    /// for the same reader and message leaves the first record.
    pub fn record_ledger(&self, f: &LedgerFacts) -> io::Result<bool> {
        let every: Vec<IndexedLink> = f
            .link_hosts
            .iter()
            .map(|h| IndexedLink {
                host: h.clone(),
                reg: h.clone(),
            })
            .collect();
        self.record_ledger_indexed(f, &every, None)
    }

    /// [`Self::record_ledger`], with link rows for `indexed_hosts` only, each
    /// kept until `links_until` (epoch seconds) when given, for
    /// [`Self::prune_expired_links`] to remove; none at all when that has
    /// already passed. Not a key TTL: an expired key leaves its entry in the
    /// table's index, so the rows are deleted, which keeps the index in step.
    ///
    /// The message row keeps every host: that is the fact. A link row is an
    /// index for re-judging, kept for the hosts and the span where finding
    /// mail by host is worth its memory.
    pub fn record_ledger_indexed(
        &self,
        f: &LedgerFacts,
        indexed_hosts: &[IndexedLink],
        links_until: Option<i64>,
    ) -> io::Result<bool> {
        let now = chrono::Utc::now().timestamp();
        let indexed: &[IndexedLink] = match links_until {
            Some(until) if until <= now => &[],
            _ => indexed_hosts,
        };
        let (user, mid) = (f.user.as_str(), f.message_id.as_str());
        let at = f.arrived.to_string();
        self.store()
            .atomic(|ctx| {
                let key = msg_key(user, mid);
                if ctx.hexists(key.as_bytes(), b"mid")? {
                    return Ok(false);
                }
                ctx.hset(
                    key.as_bytes(),
                    &[
                        (b"id" as &[u8], &key.as_bytes()[4..]),
                        (b"mid", mid.as_bytes()),
                        (b"user", f.user.as_bytes()),
                        (b"arrived", at.as_bytes()),
                        (b"from_domain", f.from_domain.as_bytes()),
                        (b"from_addr", f.from_addr.as_bytes()),
                        (b"reg", f.from_reg.as_bytes()),
                        (b"reply_to", f.reply_to.as_bytes()),
                        (b"auth", f.auth.as_bytes()),
                        (b"links", f.link_hosts.join(" ").as_bytes()),
                        (b"files", f.attachment_sha256.join(" ").as_bytes()),
                    ],
                )?;
                let until = links_until.map(|u| u.to_string());
                for link in indexed {
                    let k = link_key(user, mid, &link.host);
                    let mut row: Vec<(&[u8], &[u8])> = vec![
                        (b"id" as &[u8], &k.as_bytes()[4..]),
                        (b"host", link.host.as_bytes()),
                        (b"reg", link.reg.as_bytes()),
                    ];
                    if let Some(u) = &until {
                        row.push((b"until", u.as_bytes()));
                    }
                    ctx.hset(k.as_bytes(), &row)?;
                }
                for sha in &f.attachment_sha256 {
                    let k = file_key(user, mid, sha);
                    ctx.hset(
                        k.as_bytes(),
                        &[
                            (b"id" as &[u8], &k.as_bytes()[4..]),
                            (b"sha", sha.as_bytes()),
                        ],
                    )?;
                }
                Ok(true)
            })
            .map_err(io::Error::from)
    }

    /// Delete up to `limit` link rows kept until `now` or earlier; how many.
    /// Nothing due is one index read and no write.
    pub fn prune_expired_links(&self, now: i64, limit: usize) -> io::Result<usize> {
        let (hits, _) = self.store().idx_query(
            b"ledger_link.until",
            &kevy_embedded::IndexValue::I64(i64::MIN),
            &kevy_embedded::IndexValue::I64(now),
            None,
            limit,
        )?;
        if hits.is_empty() {
            return Ok(0);
        }
        let keys: Vec<&[u8]> = hits.iter().map(|(k, _)| k.as_slice()).collect();
        Ok(self.store().del(&keys)?)
    }

    /// Whether `user`'s copy of a message is in the ledger.
    pub fn ledger_has(&self, user: &str, mid: &str) -> io::Result<bool> {
        self.store()
            .hexists(msg_key(user, mid).as_bytes(), b"mid")
            .map_err(io::Error::from)
    }

    /// Replaces the verdict on `user`'s copy of a message.
    pub fn record_verdict(&self, user: &str, mid: &str, v: &LedgerVerdict) -> io::Result<()> {
        let key = verdict_key(user, mid);
        self.store()
            .hset(
                key.as_bytes(),
                &[
                    (b"rules" as &[u8], v.rules.join(" ").as_bytes()),
                    (b"score", v.score.to_string().as_bytes()),
                    (b"version", v.rules_version.as_bytes()),
                    (b"at", v.judged_at.to_string().as_bytes()),
                    (b"holds", flag(v.holds)),
                    (b"screening", v.screening.join(" ").as_bytes()),
                ],
            )
            .map(|_| ())
            .map_err(io::Error::from)
    }

    /// Counts one message `user` received from `addr` into their running
    /// relationship: when it began, how many have come, the name last used.
    /// An aggregate of delivery events, not a fact of its own.
    pub fn record_inbound_relationship(
        &self,
        user: &str,
        addr: &str,
        name: &str,
        at: i64,
    ) -> io::Result<()> {
        let key = format!("L:r:{user}:{addr}");
        self.store()
            .atomic(|ctx| {
                if !ctx.hexists(key.as_bytes(), b"first_at")? {
                    ctx.hset(
                        key.as_bytes(),
                        &[(b"first_at" as &[u8], at.to_string().as_bytes())],
                    )?;
                }
                ctx.hincrby(key.as_bytes(), b"in", 1)?;
                ctx.hset(key.as_bytes(), &[(b"name" as &[u8], name.as_bytes())])?;
                Ok(())
            })
            .map_err(io::Error::from)
    }

    /// Forgets `user`'s copy of a message: its facts, their rows and its verdict.
    pub fn delete_ledger(&self, user: &str, mid: &str) -> io::Result<()> {
        self.store()
            .atomic(|ctx| {
                let key = msg_key(user, mid);
                let read = |v: Option<Vec<u8>>| v.map(|v| String::from_utf8_lossy(&v).into_owned());
                let links = read(ctx.hget(key.as_bytes(), b"links")?).unwrap_or_default();
                let files = read(ctx.hget(key.as_bytes(), b"files")?).unwrap_or_default();
                let mut doomed = vec![key.clone(), verdict_key(user, mid)];
                doomed.extend(links.split_whitespace().map(|h| link_key(user, mid, h)));
                doomed.extend(files.split_whitespace().map(|s| file_key(user, mid, s)));
                let refs: Vec<&[u8]> = doomed.iter().map(|k| k.as_bytes()).collect();
                ctx.del(&refs);
                Ok(())
            })
            .map_err(io::Error::from)
    }
}

/// A reader's running relationship with one correspondent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Relationship {
    pub first_at: i64,
    pub inbound: i64,
    pub name: String,
}

fn flag(on: bool) -> &'static [u8] {
    match on {
        true => b"1",
        false => b"0",
    }
}

fn text(v: &[u8]) -> String {
    String::from_utf8_lossy(v).into_owned()
}

fn words(v: &[u8]) -> Vec<String> {
    text(v).split_whitespace().map(str::to_string).collect()
}

fn number<T: std::str::FromStr + Default>(v: &[u8]) -> T {
    text(v).parse().unwrap_or_default()
}

impl LedgerFacts {
    fn from_pairs(pairs: &[(Vec<u8>, Vec<u8>)]) -> Self {
        let mut f = Self::default();
        for (k, v) in pairs {
            match text(k).as_str() {
                "mid" => f.message_id = text(v),
                "user" => f.user = text(v),
                "from_addr" => f.from_addr = text(v),
                "from_domain" => f.from_domain = text(v),
                "reg" => f.from_reg = text(v),
                "reply_to" => f.reply_to = text(v),
                "auth" => f.auth = text(v),
                "arrived" => f.arrived = number(v),
                "links" => f.link_hosts = words(v),
                "files" => f.attachment_sha256 = words(v),
                _ => {}
            }
        }
        f
    }
}

impl LedgerVerdict {
    fn from_pairs(pairs: &[(Vec<u8>, Vec<u8>)]) -> Self {
        let mut d = Self::default();
        for (k, v) in pairs {
            match text(k).as_str() {
                "rules" => d.rules = words(v),
                "score" => d.score = number(v),
                "version" => d.rules_version = text(v),
                "at" => d.judged_at = number(v),
                "holds" => d.holds = v.as_slice() == b"1",
                "screening" => d.screening = words(v),
                _ => {}
            }
        }
        d
    }
}

impl Relationship {
    fn from_pairs(pairs: &[(Vec<u8>, Vec<u8>)]) -> Self {
        let mut r = Self::default();
        for (k, v) in pairs {
            match text(k).as_str() {
                "first_at" => r.first_at = number(v),
                "in" => r.inbound = number(v),
                "name" => r.name = text(v),
                _ => {}
            }
        }
        r
    }
}

impl KevyMailboxStore {
    /// What `user`'s copy of a message was, as delivery recorded it.
    pub fn ledger_facts(&self, user: &str, mid: &str) -> io::Result<Option<LedgerFacts>> {
        let pairs = self.store().hgetall(msg_key(user, mid).as_bytes())?;
        Ok((!pairs.is_empty()).then(|| LedgerFacts::from_pairs(&pairs)))
    }

    /// The verdict on `user`'s copy of a message, when one was recorded.
    pub fn ledger_verdict(&self, user: &str, mid: &str) -> io::Result<Option<LedgerVerdict>> {
        let pairs = self.store().hgetall(verdict_key(user, mid).as_bytes())?;
        Ok((!pairs.is_empty()).then(|| LedgerVerdict::from_pairs(&pairs)))
    }

    /// `user`'s relationship with `addr`, when they have had one.
    pub fn relationship(&self, user: &str, addr: &str) -> io::Result<Option<Relationship>> {
        let pairs = self
            .store()
            .hgetall(format!("L:r:{user}:{addr}").as_bytes())?;
        Ok((!pairs.is_empty()).then(|| Relationship::from_pairs(&pairs)))
    }
}

#[cfg(test)]
mod tests;
