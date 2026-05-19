use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static SEQUENCE: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone)]
pub struct Maildir {
    root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MessageId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Flag {
    Draft,
    Flagged,
    Passed,
    Replied,
    Seen,
    Trashed,
}

#[derive(Debug)]
pub struct Entry {
    pub id: MessageId,
    pub path: PathBuf,
    pub flags: Vec<Flag>,
}

impl Flag {
    pub fn as_char(self) -> char {
        match self {
            Flag::Draft => 'D',
            Flag::Flagged => 'F',
            Flag::Passed => 'P',
            Flag::Replied => 'R',
            Flag::Seen => 'S',
            Flag::Trashed => 'T',
        }
    }

    pub fn from_char(c: char) -> Option<Self> {
        match c {
            'D' => Some(Flag::Draft),
            'F' => Some(Flag::Flagged),
            'P' => Some(Flag::Passed),
            'R' => Some(Flag::Replied),
            'S' => Some(Flag::Seen),
            'T' => Some(Flag::Trashed),
            _ => None,
        }
    }
}

/// parse flags from the ":2,FLAGS" suffix of a maildir filename
pub fn parse_flags(info: &str) -> Vec<Flag> {
    // format: ":2,FLAGS" where FLAGS is a sorted string of flag chars
    let flags_str = info
        .strip_prefix(":2,")
        .unwrap_or("");
    let mut flags: Vec<Flag> = flags_str
        .chars()
        .filter_map(Flag::from_char)
        .collect();
    flags.sort();
    flags.dedup();
    flags
}

/// serialize flags to the ":2,FLAGS" suffix format
pub fn serialize_flags(flags: &[Flag]) -> String {
    let mut sorted: Vec<Flag> = flags.to_vec();
    sorted.sort();
    sorted.dedup();
    let chars: String = sorted.iter().map(|f| f.as_char()).collect();
    format!(":2,{chars}")
}

/// add a flag to an existing info string, returning the new info string
pub fn add_flag(info: &str, flag: Flag) -> String {
    let mut flags = parse_flags(info);
    if !flags.contains(&flag) {
        flags.push(flag);
    }
    serialize_flags(&flags)
}

impl Maildir {
    /// create a new Maildir at the given path, creating tmp/new/cur directories
    pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        let root = path.as_ref().to_path_buf();
        fs::create_dir_all(root.join("tmp"))?;
        fs::create_dir_all(root.join("new"))?;
        fs::create_dir_all(root.join("cur"))?;
        Ok(Self { root })
    }

    /// open an existing Maildir (does not create directories)
    pub fn open(path: impl AsRef<Path>) -> Self {
        Self {
            root: path.as_ref().to_path_buf(),
        }
    }

    /// deliver a message atomically: write to tmp/, then rename to new/
    pub fn deliver(&self, data: &[u8]) -> io::Result<MessageId> {
        let filename = self.generate_filename();
        let tmp_path = self.root.join("tmp").join(&filename);
        let new_path = self.root.join("new").join(&filename);

        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);

        fs::rename(&tmp_path, &new_path)?;
        Ok(MessageId(filename))
    }

    /// generate a unique maildir filename: {timestamp}.M{micros}P{pid}Q{seq}.{hostname}
    fn generate_filename(&self) -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let secs = now.as_secs();
        let micros = now.subsec_micros();
        let pid = std::process::id();
        let seq = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "localhost".to_string());
        format!("{secs}.M{micros}P{pid}Q{seq}.{hostname}")
    }

    /// scan the new/ directory for unprocessed messages
    pub fn scan_new(&self) -> io::Result<Vec<Entry>> {
        self.scan_dir("new")
    }

    /// scan the cur/ directory for processed messages
    pub fn scan_cur(&self) -> io::Result<Vec<Entry>> {
        self.scan_dir("cur")
    }

    fn scan_dir(&self, subdir: &str) -> io::Result<Vec<Entry>> {
        let dir = self.root.join(subdir);
        let mut entries = Vec::new();

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");

            let (id_part, flags) = if let Some(colon_pos) = filename.find(':') {
                let id = &filename[..colon_pos];
                let info = &filename[colon_pos..];
                (id.to_string(), parse_flags(info))
            } else {
                (filename.to_string(), vec![])
            };

            entries.push(Entry {
                id: MessageId(id_part),
                path,
                flags,
            });
        }

        Ok(entries)
    }

    /// clean up stale files in tmp/ older than the given duration
    pub fn cleanup_tmp(&self, max_age: Duration) -> io::Result<u32> {
        let tmp_dir = self.root.join("tmp");
        let mut cleaned = 0u32;
        let now = SystemTime::now();

        for entry in fs::read_dir(tmp_dir)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if !metadata.is_file() {
                continue;
            }
            if let Ok(modified) = metadata.modified()
                && let Ok(age) = now.duration_since(modified)
                    && age > max_age {
                        fs::remove_file(entry.path())?;
                        cleaned += 1;
                    }
        }

        Ok(cleaned)
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests;
