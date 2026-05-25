#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Maildir filesystem format helpers.
//!
//! `mailrs-maildir` implements the [Maildir] mail storage convention
//! invented by Daniel J. Bernstein for qmail and adopted by Dovecot,
//! Courier, mutt, neomutt, postfix, and many others. Messages are stored
//! as one file per message under `<root>/{tmp,new,cur}/`, with the
//! filename encoding a globally unique ID plus a flag suffix.
//!
//! This crate gives you the primitives — atomic delivery into `new/`,
//! directory scans, flag parsing/serialization, and `tmp/` cleanup —
//! without any mailbox-database, indexing, or IMAP/POP3 logic on top.
//!
//! This crate underpins the message storage in [mailrs], a Rust mail
//! server, and is published independently so other Rust projects can
//! reuse the Maildir layer.
//!
//! # Quick start
//!
//! ```no_run
//! use mailrs_maildir::Maildir;
//!
//! let md = Maildir::create("/var/mail/alice/INBOX").unwrap();
//! let id = md.deliver(b"From: a@example.com\r\nSubject: hi\r\n\r\nbody\r\n").unwrap();
//! for entry in md.scan_new().unwrap() {
//!     println!("{} flags={:?}", entry.id, entry.flags);
//! }
//! ```
//!
//! # What this crate does
//!
//! - **Atomic delivery**: [`Maildir::deliver`] writes to `tmp/`, fsyncs,
//!   then renames into `new/` — the standard Maildir reliability
//!   technique.
//! - **Directory scans**: [`Maildir::scan_new`] and [`Maildir::scan_cur`]
//!   list messages in each stage with their parsed flags.
//! - **Filename parsing**: [`parse_flags`] / [`serialize_flags`] /
//!   [`add_flag`] handle the `":2,FLAGS"` suffix convention.
//! - **Janitorial**: [`Maildir::cleanup_tmp`] removes stale partial
//!   deliveries.
//!
//! # What this crate does NOT do
//!
//! - No IMAP / POP3 protocol layer. See `mailrs-imap-proto`.
//! - No mailbox indexing or message search. The `cur/`-vs-`new/` split
//!   is the only state — there's no UID database here.
//! - No locking. Maildir is designed to be lock-free for delivery
//!   (atomic rename) and stage transitions (atomic rename).
//!
//! [Maildir]: https://cr.yp.to/proto/maildir.html
//! [mailrs]: https://github.com/goliajp/mailrs

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static SEQUENCE: AtomicU32 = AtomicU32::new(0);

/// A handle to a Maildir directory. Cheap to clone — only holds a path.
#[derive(Debug, Clone)]
pub struct Maildir {
    root: PathBuf,
}

/// Globally unique identifier of a delivered message, derived from the
/// filename in `new/` or `cur/` (the part before the `:2,FLAGS` suffix).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MessageId(pub String);

/// Standard Maildir flag, as defined by the Maildir specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Flag {
    /// `D` — draft message.
    Draft,
    /// `F` — flagged / starred.
    Flagged,
    /// `P` — passed / forwarded.
    Passed,
    /// `R` — replied to.
    Replied,
    /// `S` — seen / read.
    Seen,
    /// `T` — trashed (typically expunged at next IMAP EXPUNGE).
    Trashed,
}

/// One scanned message entry from `new/` or `cur/`.
#[derive(Debug)]
pub struct Entry {
    /// Identifier (filename without the `:2,FLAGS` suffix).
    pub id: MessageId,
    /// Absolute path on disk.
    pub path: PathBuf,
    /// Flags parsed from the filename suffix.
    pub flags: Vec<Flag>,
}

impl Flag {
    /// Single-character representation used in the filename suffix.
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

    /// Parse a single flag character; returns `None` for unknown letters.
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

/// Parse flags from the `":2,FLAGS"` suffix of a Maildir filename.
/// Returns a sorted, deduplicated `Vec<Flag>`.
pub fn parse_flags(info: &str) -> Vec<Flag> {
    // format: ":2,FLAGS" where FLAGS is a sorted string of flag chars
    let flags_str = info.strip_prefix(":2,").unwrap_or("");
    let mut flags: Vec<Flag> = flags_str.chars().filter_map(Flag::from_char).collect();
    flags.sort();
    flags.dedup();
    flags
}

/// Serialize flags to the `":2,FLAGS"` suffix format. Flags are sorted
/// and deduplicated for a canonical representation.
pub fn serialize_flags(flags: &[Flag]) -> String {
    let mut sorted: Vec<Flag> = flags.to_vec();
    sorted.sort();
    sorted.dedup();
    let chars: String = sorted.iter().map(|f| f.as_char()).collect();
    format!(":2,{chars}")
}

/// Add a flag to an existing `:2,FLAGS` info string, returning the new
/// info string. No-op if `flag` is already present.
pub fn add_flag(info: &str, flag: Flag) -> String {
    let mut flags = parse_flags(info);
    if !flags.contains(&flag) {
        flags.push(flag);
    }
    serialize_flags(&flags)
}

/// Process-wide cache of "paths whose tmp/new/cur we've already
/// ensured exist". Populated lazily by [`Maildir::create_cached`].
///
/// **Why a static is OK here:** the set of Maildir paths a single
/// process touches is bounded by the user count, which is bounded
/// by the underlying storage anyway. Memory is one `PathBuf` per
/// distinct user, ~few hundred KiB at the high end.
static ENSURED_PATHS: std::sync::LazyLock<dashmap::DashMap<PathBuf, ()>> =
    std::sync::LazyLock::new(dashmap::DashMap::new);

impl Maildir {
    /// Create a Maildir at the given path, creating `tmp/`, `new/`, and
    /// `cur/` if they don't already exist.
    pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        let root = path.as_ref().to_path_buf();
        fs::create_dir_all(root.join("tmp"))?;
        fs::create_dir_all(root.join("new"))?;
        fs::create_dir_all(root.join("cur"))?;
        Ok(Self { root })
    }

    /// Like [`Maildir::create`] but caches the per-path
    /// "tmp/new/cur ensured" outcome in a process-wide
    /// `DashMap<PathBuf, ()>`. The first call per `path` does the
    /// usual three `create_dir_all` syscalls; every subsequent
    /// call for the same path is a single DashMap lookup
    /// (~10 ns) and skips the syscalls entirely.
    ///
    /// **When to use:** inbound delivery hot paths where the same
    /// user receives many messages — the per-message
    /// `create_dir_all` is the same 12-syscall storm wasted every
    /// time. Profile data on mailrs's inbound SMTP path showed
    /// `stat` + `DirBuilder::_create` taking ~37% of CPU before
    /// this cache existed (samply, 2026-05-24).
    ///
    /// **When NOT to use:** one-off setup paths (admin tool
    /// creating a new user's mailbox, test setup, etc.) —
    /// [`Maildir::create`] is fine there and never hits a cache
    /// miss penalty.
    ///
    /// Cache entries never expire. If you intentionally delete a
    /// user's Maildir directory while the process is running,
    /// call [`Maildir::invalidate_cache`] to drop the stale
    /// "ensured" marker.
    pub fn create_cached(path: impl AsRef<Path>) -> io::Result<Self> {
        let root = path.as_ref().to_path_buf();
        if ENSURED_PATHS.contains_key(&root) {
            // Fast path — directories already known to exist.
            return Ok(Self { root });
        }
        // Slow path — first sight of this path; do the syscalls.
        fs::create_dir_all(root.join("tmp"))?;
        fs::create_dir_all(root.join("new"))?;
        fs::create_dir_all(root.join("cur"))?;
        ENSURED_PATHS.insert(root.clone(), ());
        Ok(Self { root })
    }

    /// Drop the cached "tmp/new/cur ensured" marker for `path`.
    /// Call after manually removing or recreating a Maildir on
    /// disk so the next [`Maildir::create_cached`] call re-runs
    /// the `create_dir_all` syscalls.
    pub fn invalidate_cache(path: impl AsRef<Path>) {
        ENSURED_PATHS.remove(path.as_ref());
    }

    /// Open an existing Maildir. Does not check that the subdirectories
    /// exist — use [`Maildir::create`] for guaranteed setup.
    pub fn open(path: impl AsRef<Path>) -> Self {
        Self {
            root: path.as_ref().to_path_buf(),
        }
    }

    /// Atomically deliver a message: write the body to `tmp/`, fsync,
    /// then rename to `new/`. Returns the generated [`MessageId`].
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

    /// Atomically deliver N messages, batching the durability sync to
    /// amortize fsync cost across the whole batch.
    ///
    /// **When to use:** N ≥ 4. Measured on APFS (Apple M-series,
    /// 2026-05-24, `benches/deliver.rs`):
    /// - N=1:  **0.51×** speed (batch is SLOWER — use [`Self::deliver`])
    /// - N=8:  **3.88×** speed
    /// - N=64: **15.27×** speed (3700 msg/s ceiling at this batch size)
    ///
    /// The crossover happens at N≈2 because the batch always pays
    /// for 2 directory fsyncs regardless of N, while N × per-file
    /// [`Self::deliver`] pays 1 fsync each. With only one message
    /// the extra directory fsync is pure overhead.
    ///
    /// Strategy:
    /// 1. Write all messages to `tmp/{filename}` (no per-file fsync).
    /// 2. **One** fsync on the `tmp/` directory — commits both the
    ///    directory entries and (on APFS / ext4 / btrfs) the contained
    ///    file data. For paranoid portability across less common
    ///    filesystems consider [`Self::deliver`] in a loop instead.
    /// 3. Rename each tmp → new.
    /// 4. **One** fsync on the `new/` directory — commits all renames.
    ///
    /// Total fsync syscalls: **2** (instead of N for the per-message
    /// path). On a modern NVMe SSD where the wait-for-write-barrier
    /// dominates per-fsync cost, batching N=10 messages can be
    /// 5-10× faster wall-clock than N × `deliver()`.
    ///
    /// **Atomicity:** each individual rename is atomic. The batch as a
    /// whole is NOT transactional — if the process crashes between
    /// steps 3 and 4, some messages will be visible in `new/`
    /// (durable per step 3 implicitly) and others may still be in
    /// `tmp/` (cleanup via [`Self::cleanup_tmp`]). No messages are
    /// ever lost or duplicated.
    ///
    /// **Order:** returned `MessageId`s correspond positionally to the
    /// input slice (`ids[i]` is the id of `messages[i]`).
    ///
    /// **Empty input:** returns `Ok(Vec::new())` without any syscalls.
    pub fn deliver_batch(&self, messages: &[&[u8]]) -> io::Result<Vec<MessageId>> {
        if messages.is_empty() {
            return Ok(Vec::new());
        }
        let tmp_dir = self.root.join("tmp");
        let new_dir = self.root.join("new");

        // Phase 1: write all messages to tmp/ (no per-file fsync).
        let mut ids = Vec::with_capacity(messages.len());
        let mut tmp_paths = Vec::with_capacity(messages.len());
        for msg in messages {
            let filename = self.generate_filename();
            let tmp_path = tmp_dir.join(&filename);
            let mut file = fs::File::create(&tmp_path)?;
            file.write_all(msg)?;
            // Deliberately NOT calling file.sync_all() — the dir
            // fsync below covers it.
            drop(file);
            ids.push(MessageId(filename));
            tmp_paths.push(tmp_path);
        }

        // Phase 2: ONE fsync on tmp/. On APFS this implicitly flushes
        // the newly-written file contents because APFS journals
        // directory + inode updates together; same for ext4 with
        // data=ordered (default) and for btrfs. On other filesystems
        // (XFS, ZFS) data may not be durable until a subsequent
        // file-level fsync — callers worried about that should use
        // `deliver()` in a loop.
        fs::File::open(&tmp_dir)?.sync_all()?;

        // Phase 3: rename all tmp → new.
        for (id, tmp_path) in ids.iter().zip(tmp_paths.iter()) {
            let new_path = new_dir.join(&id.0);
            fs::rename(tmp_path, &new_path)?;
        }

        // Phase 4: ONE fsync on new/ to durable the renames.
        fs::File::open(&new_dir)?.sync_all()?;

        Ok(ids)
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

    /// Scan `new/` for unprocessed messages — the messages a delivery
    /// agent has just dropped off but no client has acknowledged yet.
    pub fn scan_new(&self) -> io::Result<Vec<Entry>> {
        self.scan_dir("new")
    }

    /// Scan `cur/` for processed messages — once a client has read a
    /// message it should be moved from `new/` to `cur/` (and the
    /// filename suffix updated with the new flags).
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
            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

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

    /// Remove files in `tmp/` older than `max_age`. These are leftover
    /// partial deliveries from crashed processes. Returns the number of
    /// files removed.
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
                && age > max_age
            {
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
