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

const MSG_PREFIX: &[u8] = b"L:m:";
const LINK_PREFIX: &[u8] = b"L:l:";
const FILE_PREFIX: &[u8] = b"L:f:";

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
}

/// What the rules made of a message: derived, recomputed at will.
#[derive(Debug, Clone, Default)]
pub struct LedgerVerdict {
    pub rules: Vec<String>,
    pub score: f64,
    pub rules_version: String,
    pub judged_at: i64,
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
            ],
            &["from_domain", "from_addr"],
        ),
        table(
            "ledger_link",
            LINK_PREFIX,
            vec![col("id", ValType::Str), col("host", ValType::Str)],
            &["host"],
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

    /// Records what a delivered message was, for its reader. Written once: a
    /// second call for the same reader and message leaves the first record.
    pub fn record_ledger(&self, f: &LedgerFacts) -> io::Result<bool> {
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
                        (b"reply_to", f.reply_to.as_bytes()),
                        (b"auth", f.auth.as_bytes()),
                        (b"links", f.link_hosts.join(" ").as_bytes()),
                        (b"files", f.attachment_sha256.join(" ").as_bytes()),
                    ],
                )?;
                for host in &f.link_hosts {
                    let k = link_key(user, mid, host);
                    ctx.hset(
                        k.as_bytes(),
                        &[
                            (b"id" as &[u8], &k.as_bytes()[4..]),
                            (b"host", host.as_bytes()),
                        ],
                    )?;
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

    /// The messages from a sender domain, as `(message id, user)`.
    pub fn ledger_by_domain(&self, domain: &str) -> io::Result<Vec<(String, String)>> {
        let rows = self.ledger_hits(b"ledger_msg.from_domain", domain)?;
        self.ledger_messages(rows.into_iter().map(|k| k[MSG_PREFIX.len()..].to_string()))
    }

    /// The messages that linked to a host.
    pub fn ledger_by_link_host(&self, host: &str) -> io::Result<Vec<(String, String)>> {
        self.ledger_by_part(b"ledger_link.host", LINK_PREFIX, host)
    }

    /// The messages that carried a file.
    pub fn ledger_by_attachment(&self, sha256: &str) -> io::Result<Vec<(String, String)>> {
        self.ledger_by_part(b"ledger_file.sha", FILE_PREFIX, sha256)
    }

    /// The messages a link or file row points to: its id is the message
    /// row's id, `|`, then the value it is keyed on.
    fn ledger_by_part(
        &self,
        index: &[u8],
        prefix: &[u8],
        value: &str,
    ) -> io::Result<Vec<(String, String)>> {
        let suffix = format!("|{value}");
        let rows = self.ledger_hits(index, value)?;
        self.ledger_messages(
            rows.into_iter()
                .filter_map(|k| k[prefix.len()..].strip_suffix(&suffix).map(str::to_string)),
        )
    }

    /// The keys of the rows an index holds for `value`.
    fn ledger_hits(&self, index: &[u8], value: &str) -> io::Result<Vec<String>> {
        let v = kevy_embedded::IndexValue::Str(value.as_bytes().to_vec());
        let mut out = Vec::new();
        let mut cursor = None;
        loop {
            let (hits, next) = self
                .store()
                .idx_query(index, &v, &v, cursor.as_ref(), 10_000)?;
            out.extend(
                hits.into_iter()
                    .map(|(k, _)| String::from_utf8_lossy(&k).into_owned()),
            );
            match next {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        Ok(out)
    }

    /// `(message id, user)` of each message row named by its id.
    fn ledger_messages(
        &self,
        ids: impl Iterator<Item = String>,
    ) -> io::Result<Vec<(String, String)>> {
        let mut out = Vec::new();
        for id in ids {
            let key = format!("L:m:{id}");
            let read = |field: &[u8]| -> io::Result<String> {
                Ok(self
                    .store()
                    .hget(key.as_bytes(), field)?
                    .map(|v| String::from_utf8_lossy(&v).into_owned())
                    .unwrap_or_default())
            };
            out.push((read(b"mid")?, read(b"user")?));
        }
        Ok(out)
    }
}

/// A reader's running relationship with one correspondent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Relationship {
    pub first_at: i64,
    pub inbound: i64,
    pub name: String,
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
