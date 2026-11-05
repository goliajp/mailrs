//! Each delivered message's template fingerprint, once per message rather
//! than once per reader: a message sent to thirty people is one row.
//!
//! The row (`L:t:{mid}`) is a derivation — the SimHash of the body and the
//! link domains, attachments and sender domain it is compared with — and
//! can be computed again from the file. Finding candidates needs the hash's
//! four 16-bit pieces indexed; that index (`L:ts:{mid}`) is kept only for
//! [`SEGMENT_DAYS`], the span a report looks back over, and pruned after.
//! Both go when the account purge removes the message's last reader.

use std::io;

use kevy_embedded::{IndexKind, IndexValType as ValType, IndexValue, TableIndex, TableSpec};
use mailrs_report::Template;

use crate::KevyMailboxStore;

pub(crate) const TPL_PREFIX: &[u8] = b"L:t:";
pub(crate) const SEG_PREFIX: &[u8] = b"L:ts:";

/// How long a message stays findable by its fingerprint, in days.
pub const SEGMENT_DAYS: i64 = 30;

const SEG_COLUMNS: [&str; 4] = ["s0", "s1", "s2", "s3"];

pub(crate) fn specs() -> [TableSpec; 2] {
    let bytes = |n: &str| n.as_bytes().to_vec();
    let range = |c: &str| TableIndex {
        column: bytes(c),
        kind: IndexKind::Range,
        values: Vec::new(),
    };
    let base =
        |name: &str, prefix: &[u8], columns: Vec<(Vec<u8>, ValType)>, idx: Vec<TableIndex>| {
            TableSpec {
                name: bytes(name),
                prefix: prefix.to_vec(),
                pk: bytes("id"),
                window: None,
                autodeclare: 0,
                auto_added: Vec::new(),
                columns,
                indexes: idx,
                orderpaths: Vec::new(),
            }
        };
    let mut seg_cols = vec![(bytes("id"), ValType::Str), (bytes("at"), ValType::I64)];
    seg_cols.extend(SEG_COLUMNS.iter().map(|c| (bytes(c), ValType::I64)));
    let mut seg_idx: Vec<TableIndex> = SEG_COLUMNS.iter().map(|c| range(c)).collect();
    seg_idx.push(range("at"));
    [
        base(
            "ledger_tpl",
            TPL_PREFIX,
            vec![(bytes("id"), ValType::Str), (bytes("at"), ValType::I64)],
            Vec::new(),
        ),
        base("ledger_tpl_seg", SEG_PREFIX, seg_cols, seg_idx),
    ]
}

fn tpl_key(mid: &str) -> String {
    format!("L:t:{mid}")
}
fn seg_key(mid: &str) -> String {
    format!("L:ts:{mid}")
}

fn text(v: Option<Vec<u8>>) -> String {
    v.map(|v| String::from_utf8_lossy(&v).into_owned())
        .unwrap_or_default()
}

/// A stored fingerprint, with when its message arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTemplate {
    pub template: Template,
    pub at: i64,
}

impl KevyMailboxStore {
    /// Records a message's fingerprint the first time it is delivered;
    /// later deliveries of the same message leave it. True when written.
    pub fn record_template(&self, mid: &str, t: &Template, at: i64) -> io::Result<bool> {
        let now = chrono::Utc::now().timestamp();
        let findable = at > now - SEGMENT_DAYS * 86_400;
        let hash = format!("{:016x}", t.simhash);
        let at_s = at.to_string();
        let pieces = mailrs_report::fingerprint::segments(t.simhash).map(|p| p.to_string());
        self.store()
            .atomic(|ctx| {
                let key = tpl_key(mid);
                if ctx.hexists(key.as_bytes(), b"hash")? {
                    return Ok(false);
                }
                ctx.hset(
                    key.as_bytes(),
                    &[
                        (b"id" as &[u8], mid.as_bytes()),
                        (b"at", at_s.as_bytes()),
                        (b"hash", hash.as_bytes()),
                        (b"links", t.link_domains.join(" ").as_bytes()),
                        (b"files", t.attachments.join(" ").as_bytes()),
                        (b"sender", t.sender_domain.as_bytes()),
                    ],
                )?;
                if findable {
                    let k = seg_key(mid);
                    let mut row: Vec<(&[u8], &[u8])> =
                        vec![(b"id" as &[u8], mid.as_bytes()), (b"at", at_s.as_bytes())];
                    for (c, v) in SEG_COLUMNS.iter().zip(pieces.iter()) {
                        row.push((c.as_bytes(), v.as_bytes()));
                    }
                    ctx.hset(k.as_bytes(), &row)?;
                }
                Ok(true)
            })
            .map_err(io::Error::from)
    }

    /// Removes a message's fingerprint and segment row once no reader has a
    /// ledger row for it; true when removed.
    pub fn forget_template_if_unread(&self, mid: &str) -> io::Result<bool> {
        if !self.ledger_readers(mid)?.is_empty() {
            return Ok(false);
        }
        let (t, s) = (tpl_key(mid), seg_key(mid));
        Ok(self.store().del(&[t.as_bytes(), s.as_bytes()])? > 0)
    }

    /// The fingerprint recorded for a message, if any.
    pub fn template_of(&self, mid: &str) -> io::Result<Option<StoredTemplate>> {
        let row = self.store().hgetall(tpl_key(mid).as_bytes())?;
        let mut hash = None;
        let mut t = StoredTemplate {
            template: Template::default(),
            at: 0,
        };
        let words = |v: &[u8]| {
            String::from_utf8_lossy(v)
                .split_whitespace()
                .map(str::to_string)
                .collect()
        };
        for (f, v) in &row {
            match f.as_slice() {
                b"hash" => hash = Some(String::from_utf8_lossy(v).into_owned()),
                b"at" => t.at = String::from_utf8_lossy(v).parse().unwrap_or(0),
                b"links" => t.template.link_domains = words(v),
                b"files" => t.template.attachments = words(v),
                b"sender" => t.template.sender_domain = String::from_utf8_lossy(v).into_owned(),
                _ => {}
            }
        }
        let Some(hash) = hash else {
            return Ok(None);
        };
        t.template.simhash = u64::from_str_radix(&hash, 16)
            .map_err(|e| io::Error::other(format!("fingerprint of {mid}: {e}")))?;
        Ok(Some(t))
    }

    /// Messages arrived at `since` or later that share at least one piece
    /// with `simhash`, `limit` per piece: every message within three bits
    /// is among them. Each once; the reported message itself included.
    pub fn template_candidates(
        &self,
        simhash: u64,
        since: i64,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        let mut out: Vec<String> = Vec::new();
        for (c, piece) in SEG_COLUMNS
            .iter()
            .zip(mailrs_report::fingerprint::segments(simhash))
        {
            let v = IndexValue::I64(i64::from(piece));
            let index = format!("ledger_tpl_seg.{c}");
            let (hits, _) = self
                .store()
                .idx_query(index.as_bytes(), &v, &v, None, limit)?;
            for (k, _) in hits {
                let mid = String::from_utf8_lossy(&k[SEG_PREFIX.len()..]).into_owned();
                if out.contains(&mid) {
                    continue;
                }
                let at = text(self.store().hget(&k, b"at")?)
                    .parse::<i64>()
                    .unwrap_or(0);
                if at >= since {
                    out.push(mid);
                }
            }
        }
        Ok(out)
    }

    /// Removes up to `limit` segment rows of messages that arrived before
    /// `before`; how many. The fingerprints stay.
    pub fn prune_template_segments(&self, before: i64, limit: usize) -> io::Result<usize> {
        let (hits, _) = self.store().idx_query(
            b"ledger_tpl_seg.at",
            &IndexValue::I64(i64::MIN),
            &IndexValue::I64(before - 1),
            None,
            limit,
        )?;
        if hits.is_empty() {
            return Ok(0);
        }
        let keys: Vec<&[u8]> = hits.iter().map(|(k, _)| k.as_slice()).collect();
        Ok(self.store().del(&keys)?)
    }

    /// The readers the ledger has a copy of `mid` for.
    pub fn ledger_readers(&self, mid: &str) -> io::Result<Vec<String>> {
        let v = IndexValue::Str(mid.as_bytes().to_vec());
        let (hits, _) = self
            .store()
            .idx_query(b"ledger_msg.mid", &v, &v, None, 10_000)?;
        let mut out = Vec::new();
        for (k, _) in hits {
            let user = text(self.store().hget(&k, b"user")?);
            if !user.is_empty() && !out.contains(&user) {
                out.push(user);
            }
        }
        Ok(out)
    }

    /// How many fingerprints and findable segment rows are stored, and the
    /// bytes their fields hold, reading at most `limit` of each.
    pub fn template_footprint(&self, limit: usize) -> io::Result<Footprint> {
        let mut f = Footprint::default();
        let all = (IndexValue::I64(i64::MIN), IndexValue::I64(i64::MAX));
        let (segs, more) =
            self.store()
                .idx_query(b"ledger_tpl_seg.at", &all.0, &all.1, None, limit)?;
        f.segment_rows = segs.len() as u64;
        f.capped = more.is_some();
        for (k, _) in &segs {
            let mid = String::from_utf8_lossy(&k[SEG_PREFIX.len()..]).into_owned();
            f.segment_bytes += field_bytes(self, k)?;
            f.fingerprint_bytes += field_bytes(self, tpl_key(&mid).as_bytes())?;
            f.fingerprints += 1;
        }
        Ok(f)
    }
}

fn field_bytes(store: &KevyMailboxStore, key: &[u8]) -> io::Result<u64> {
    Ok(store
        .store()
        .hgetall(key)?
        .iter()
        .map(|(k, v)| (k.len() + v.len()) as u64)
        .sum::<u64>()
        + key.len() as u64)
}

/// What [`KevyMailboxStore::template_footprint`] counted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Footprint {
    pub fingerprints: u64,
    pub fingerprint_bytes: u64,
    pub segment_rows: u64,
    pub segment_bytes: u64,
    /// More segment rows exist than were read.
    pub capped: bool,
}

#[cfg(test)]
mod tests;
