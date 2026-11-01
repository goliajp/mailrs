//! Finding mail in the ledger by what it was: its sender domain, a link
//! host, an attachment, and for a reputation change, a domain together with
//! every subdomain under it.

use std::io;

use super::{FILE_PREFIX, LINK_PREFIX, MSG_PREFIX};
use crate::KevyMailboxStore;

impl KevyMailboxStore {
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

    /// The messages from `domain` or any subdomain of it, as `(message id,
    /// user)`. `reg` is `domain`'s registrable domain, which every row
    /// under it is indexed by: an observation names a domain, and mail
    /// from its subdomains is mail from it.
    pub fn ledger_by_domain_tree(
        &self,
        domain: &str,
        reg: &str,
    ) -> io::Result<Vec<(String, String)>> {
        let mut ids = Vec::new();
        for key in self.ledger_hits(b"ledger_msg.reg", reg)? {
            let from = self
                .store()
                .hget(key.as_bytes(), b"from_domain")?
                .map(|v| String::from_utf8_lossy(&v).into_owned())
                .unwrap_or_default();
            if under(&from, domain) {
                ids.push(key[MSG_PREFIX.len()..].to_string());
            }
        }
        self.ledger_messages(ids.into_iter())
    }

    /// The messages with a link row for `host` or any subdomain of it, as
    /// `(message id, user)`, once each; `reg` as for
    /// [`Self::ledger_by_domain_tree`]. Only what the link retention keeps.
    pub fn ledger_by_link_tree(&self, host: &str, reg: &str) -> io::Result<Vec<(String, String)>> {
        let mut ids: Vec<String> = Vec::new();
        for key in self.ledger_hits(b"ledger_link.reg", reg)? {
            let id = &key[LINK_PREFIX.len()..];
            let Some((msg, linked)) = id.rsplit_once('|') else {
                continue;
            };
            if under(linked, host) && !ids.iter().any(|i| i == msg) {
                ids.push(msg.to_string());
            }
        }
        self.ledger_messages(ids.into_iter())
    }
}

/// `name` is `domain` or a subdomain of it.
fn under(name: &str, domain: &str) -> bool {
    let (name, domain) = (name.to_ascii_lowercase(), domain.to_ascii_lowercase());
    name == domain || name.ends_with(&format!(".{domain}"))
}
