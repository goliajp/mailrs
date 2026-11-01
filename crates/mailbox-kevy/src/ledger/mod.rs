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

fn table(
    name: &str,
    prefix: &[u8],
    pk: &str,
    cols: &[(&str, ValType)],
    keyed: &[&str],
) -> TableSpec {
    let col = |n: &str| n.as_bytes().to_vec();
    TableSpec {
        name: name.as_bytes().to_vec(),
        prefix: prefix.to_vec(),
        pk: col(pk),
        window: None,
        autodeclare: 0,
        auto_added: Vec::new(),
        columns: cols.iter().map(|(n, t)| (col(n), *t)).collect(),
        indexes: keyed
            .iter()
            .map(|k| TableIndex {
                column: col(k),
                kind: IndexKind::Range,
                values: vec![col("arrived"), col("user"), col("mid")],
            })
            .collect(),
        orderpaths: Vec::new(),
    }
}

/// The three ledger tables, as declared.
pub(crate) fn specs() -> [TableSpec; 3] {
    let common = [
        ("mid", ValType::Str),
        ("user", ValType::Str),
        ("arrived", ValType::I64),
    ];
    let with = |extra: &[(&'static str, ValType)]| {
        let mut v: Vec<(&str, ValType)> = common.to_vec();
        v.extend_from_slice(extra);
        v
    };
    [
        table(
            "ledger_msg",
            MSG_PREFIX,
            "mid",
            &with(&[("from_domain", ValType::Str), ("from_addr", ValType::Str)]),
            &["from_domain", "from_addr"],
        ),
        table(
            "ledger_link",
            LINK_PREFIX,
            "id",
            &with(&[("id", ValType::Str), ("host", ValType::Str)]),
            &["host"],
        ),
        table(
            "ledger_file",
            FILE_PREFIX,
            "id",
            &with(&[("id", ValType::Str), ("sha", ValType::Str)]),
            &["sha"],
        ),
    ]
}

fn msg_key(mid: &str) -> String {
    format!("L:m:{mid}")
}
fn link_key(mid: &str, host: &str) -> String {
    format!("L:l:{mid}|{host}")
}
fn file_key(mid: &str, sha: &str) -> String {
    format!("L:f:{mid}|{sha}")
}
fn verdict_key(mid: &str) -> String {
    format!("L:v:{mid}")
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

    /// Records what a delivered message was. Written once: a second call
    /// for the same message leaves the first record as it was.
    pub fn record_ledger(&self, f: &LedgerFacts) -> io::Result<bool> {
        let mid = f.message_id.as_str();
        let at = f.arrived.to_string();
        self.store()
            .atomic(|ctx| {
                let key = msg_key(mid);
                if ctx.hexists(key.as_bytes(), b"mid")? {
                    return Ok(false);
                }
                ctx.hset(
                    key.as_bytes(),
                    &[
                        (b"mid" as &[u8], mid.as_bytes()),
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
                    let k = link_key(mid, host);
                    ctx.hset(
                        k.as_bytes(),
                        &[
                            (b"id" as &[u8], k[4..].as_bytes()),
                            (b"mid", mid.as_bytes()),
                            (b"user", f.user.as_bytes()),
                            (b"arrived", at.as_bytes()),
                            (b"host", host.as_bytes()),
                        ],
                    )?;
                }
                for sha in &f.attachment_sha256 {
                    let k = file_key(mid, sha);
                    ctx.hset(
                        k.as_bytes(),
                        &[
                            (b"id" as &[u8], k[4..].as_bytes()),
                            (b"mid", mid.as_bytes()),
                            (b"user", f.user.as_bytes()),
                            (b"arrived", at.as_bytes()),
                            (b"sha", sha.as_bytes()),
                        ],
                    )?;
                }
                Ok(true)
            })
            .map_err(io::Error::from)
    }

    /// Replaces a message's verdict.
    pub fn record_verdict(&self, mid: &str, v: &LedgerVerdict) -> io::Result<()> {
        let key = verdict_key(mid);
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

    /// Forgets a message: its facts, their rows and its verdict.
    pub fn delete_ledger(&self, mid: &str) -> io::Result<()> {
        self.store()
            .atomic(|ctx| {
                let key = msg_key(mid);
                let read = |v: Option<Vec<u8>>| v.map(|v| String::from_utf8_lossy(&v).into_owned());
                let links = read(ctx.hget(key.as_bytes(), b"links")?).unwrap_or_default();
                let files = read(ctx.hget(key.as_bytes(), b"files")?).unwrap_or_default();
                let mut doomed = vec![key.clone(), verdict_key(mid)];
                doomed.extend(links.split_whitespace().map(|h| link_key(mid, h)));
                doomed.extend(files.split_whitespace().map(|s| file_key(mid, s)));
                let refs: Vec<&[u8]> = doomed.iter().map(|k| k.as_bytes()).collect();
                ctx.del(&refs);
                Ok(())
            })
            .map_err(io::Error::from)
    }

    /// The messages from a sender domain, as `(message id, user)`.
    pub fn ledger_by_domain(&self, domain: &str) -> io::Result<Vec<(String, String)>> {
        self.ledger_lookup(b"ledger_msg.from_domain", domain)
    }

    /// The messages that linked to a host.
    pub fn ledger_by_link_host(&self, host: &str) -> io::Result<Vec<(String, String)>> {
        self.ledger_lookup(b"ledger_link.host", host)
    }

    /// The messages that carried a file.
    pub fn ledger_by_attachment(&self, sha256: &str) -> io::Result<Vec<(String, String)>> {
        self.ledger_lookup(b"ledger_file.sha", sha256)
    }

    fn ledger_lookup(&self, index: &[u8], value: &str) -> io::Result<Vec<(String, String)>> {
        let v = kevy_embedded::IndexValue::Str(value.as_bytes().to_vec());
        let mut out = Vec::new();
        let mut cursor = None;
        loop {
            let (hits, next) = self
                .store()
                .idx_query(index, &v, &v, cursor.as_ref(), 10_000)?;
            for (key, _) in hits {
                let key = String::from_utf8_lossy(&key).into_owned();
                let mid = self
                    .store()
                    .hget(key.as_bytes(), b"mid")?
                    .map(|m| String::from_utf8_lossy(&m).into_owned())
                    .unwrap_or_default();
                let user = self
                    .store()
                    .hget(key.as_bytes(), b"user")?
                    .map(|u| String::from_utf8_lossy(&u).into_owned())
                    .unwrap_or_default();
                out.push((mid, user));
            }
            match next {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
