#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

mod detect;

use std::collections::{HashMap, HashSet};

/// One attachment, as the caller parsed it.
#[derive(Debug, Clone, Copy)]
pub struct Attachment<'a> {
    /// The file name it carries, or empty.
    pub name: &'a str,
    /// Its declared content type, lowercased, or empty.
    pub declared_type: &'a str,
    /// Its decoded bytes.
    pub bytes: &'a [u8],
}

/// How much of a part the detectors read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Bytes of one part read at most by the detectors that scan contents.
    pub max_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_bytes: 8 * 1024 * 1024,
        }
    }
}

/// What the detectors found in one attachment. Each field is what was
/// found, for the record and for the rules, or `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct PartFacts {
    /// Its position among the message's attachments, from 0.
    pub part: usize,
    /// Its name.
    pub name: String,
    /// Its extension, lowercased, or empty.
    pub extension: String,
    /// The extension when it is one the machine runs.
    pub executable_name: Option<String>,
    /// How its name hides its real extension.
    pub double_extension: Option<String>,
    /// A program or disk image named as something else.
    pub type_mismatch: Option<String>,
    /// A program named in a zip's directory.
    pub archive_executable: Option<String>,
    /// An encrypted entry of a zip.
    pub encrypted_archive: Option<String>,
    /// A rar or 7z whose contents are not listed.
    pub unlistable_archive: Option<String>,
    /// An Office macro project.
    pub office_macros: Option<String>,
    /// An embedded OLE object in an Office file.
    pub office_object: Option<String>,
    /// An embedded object in an RTF file.
    pub rtf_object: Option<String>,
    /// A PDF script, launch action, or embedded program.
    pub pdf_script: Option<String>,
    /// A PDF embedded file or open action.
    pub pdf_embedded: Option<String>,
    /// The longest base64 run in an HTML or SVG attachment.
    pub html_base64_run: usize,
    /// The code in it that writes a blob out as a file, if any.
    pub html_writer: Option<String>,
    /// The site an HTML form posts to.
    pub html_form: Option<String>,
    /// Script in an SVG.
    pub svg_script: Option<String>,
    /// The deny observation its hash matches.
    pub hash_listed: Option<HashListing>,
    /// Lowercase hex sha256.
    pub sha256: String,
    /// A detector stopped at the byte cap.
    pub truncated: bool,
}

/// Whether a hash is known bad, known good, or a mistake taken back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stance {
    /// Known bad.
    Deny,
    /// Known good: the file is left out of the facts altogether.
    Allow,
    /// Withdraws the observation named in `retracts`.
    Retract,
}

/// The deny observation a file's hash matches.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct HashListing {
    /// The observation's id.
    pub id: String,
    /// Whole days since it was made.
    pub age_days: i64,
    /// Its reason.
    pub reason: String,
}

/// One fact about one file hash, from the caller's own records.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct HashObservation {
    /// Unique id.
    pub id: String,
    /// Unix seconds.
    pub at: i64,
    /// Lowercase hex sha256 of the file.
    pub sha256: String,
    /// Deny, allow, or retract.
    pub stance: Stance,
    /// For `Retract`: the id it withdraws.
    #[serde(default)]
    pub retracts: Option<String>,
    /// Why.
    pub reason: String,
    /// Who recorded it, when a person did.
    #[serde(default)]
    pub actor: Option<String>,
}

/// The hash facts, ready to inspect with.
#[derive(Debug, Clone, Default)]
pub struct Inspector {
    limits: Limits,
    deny: HashMap<String, HashListing>,
    allow: HashSet<String>,
}

impl Inspector {
    /// Build from hash observations at `now` (unix seconds). Retractions
    /// remove what they name; an allowed hash is never denied.
    pub fn build(observations: &[HashObservation], limits: Limits, now: i64) -> Inspector {
        let retracted: HashSet<&str> = observations
            .iter()
            .filter(|o| o.stance == Stance::Retract)
            .filter_map(|o| o.retracts.as_deref())
            .collect();
        let mut deny = HashMap::new();
        let mut allow = HashSet::new();
        for o in observations
            .iter()
            .filter(|o| o.stance != Stance::Retract && !retracted.contains(o.id.as_str()))
        {
            let h = o.sha256.trim().to_ascii_lowercase();
            match o.stance {
                Stance::Allow => {
                    allow.insert(h);
                }
                _ => {
                    let listing = HashListing {
                        id: o.id.clone(),
                        age_days: (now - o.at).max(0) / 86_400,
                        reason: o.reason.clone(),
                    };
                    deny.insert(h, listing);
                }
            }
        }
        deny.retain(|h, _| !allow.contains(h));
        Inspector {
            limits,
            deny,
            allow,
        }
    }

    /// Hashes currently denied.
    pub fn denied_hashes(&self) -> usize {
        self.deny.len()
    }

    /// What every attachment holds. An allowed file is left out.
    pub fn inspect(&self, parts: &[Attachment<'_>]) -> Vec<PartFacts> {
        parts
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                let sha256 = detect::sha256_hex(a.bytes);
                if self.allow.contains(&sha256) {
                    return None;
                }
                let mut f = detect::facts(a, &self.limits);
                f.part = i;
                f.hash_listed = self.deny.get(&sha256).cloned();
                f.sha256 = sha256;
                Some(f)
            })
            .collect()
    }
}
