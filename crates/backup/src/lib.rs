//! A daily, encrypted copy of everything mailrs keeps: the core's embedded
//! kevy, the network kevy-server's data, the maildir and the DKIM keys.
//!
//! One archive per run: a tar stream, zstd-compressed and encrypted to an
//! age public key, so whoever holds the server cannot read its backups.
//! It is written under a temporary name and renamed when complete; its
//! `.sha256` is written last, and whoever copies archives off the server
//! takes only pairs whose checksum exists. `last.json` says how the last
//! run went.
//!
//! Nothing here opens a store. An AOF is append-only, so a copy of its
//! first N bytes, N its length when the run began, is a consistent prefix:
//! the last command may be torn, and replay drops a torn tail the same way
//! it does after a power cut. Maildir delivery writes a temporary file and
//! renames it into `new/`, so only `new/` and `cur/` are read.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

mod restore;
pub use restore::restore;

/// What one archive holds: each source goes under its `name` in the tar.
#[derive(Debug, Clone)]
pub struct Source {
    pub name: String,
    pub path: PathBuf,
    pub kind: SourceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// A kevy data directory; its AOFs are read as prefixes (see above).
    Kevy,
    /// A maildir tree: `tmp/` is skipped.
    Maildir,
    /// Plain files, copied whole.
    Files,
}

/// One run's settings.
#[derive(Debug, Clone)]
pub struct Plan {
    pub sources: Vec<Source>,
    /// Where archives, their checksums and `last.json` are written.
    pub target: PathBuf,
    /// The age public key archives are encrypted to.
    pub recipient: String,
    /// Archives kept, newest first.
    pub keep: usize,
}

/// How a run went, as `last.json` records it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Outcome {
    pub started: i64,
    pub finished: i64,
    pub ok: bool,
    pub bytes: u64,
    pub sha256: String,
    pub file: String,
    pub reason: String,
    /// When a run last succeeded, this one or an earlier one.
    pub last_success: Option<i64>,
}

/// Free space a run needs before it starts: this many times the last
/// archive, so a run does not fill the disk and leave half a file.
const SPACE_FACTOR: f64 = 1.5;

/// Run `plan` once at `now` (unix seconds), named for `day` (`YYYYMMDD`).
/// `free` reports the bytes available under the target. The outcome is
/// also written to `last.json`; an `Err` is a failure to write even that.
pub fn run(
    plan: &Plan,
    now: i64,
    day: &str,
    free: impl Fn(&Path) -> io::Result<u64>,
) -> io::Result<Outcome> {
    fs::create_dir_all(&plan.target)?;
    let file = format!("mailrs-{day}.tar.age");
    let before = last_outcome(&plan.target).and_then(|o| o.last_success);
    let mut outcome = match attempt(plan, &file, free) {
        Ok((bytes, sha256)) => Outcome {
            started: now,
            finished: unix_now(),
            ok: true,
            bytes,
            sha256,
            file,
            reason: String::new(),
            last_success: None,
        },
        Err(e) => Outcome {
            started: now,
            finished: unix_now(),
            ok: false,
            bytes: 0,
            sha256: String::new(),
            file,
            reason: e.to_string(),
            last_success: before,
        },
    };
    if outcome.ok {
        outcome.last_success = Some(outcome.finished);
    }
    if outcome.ok {
        prune(&plan.target, plan.keep)?;
    }
    write_atomically(
        &plan.target.join("last.json"),
        &serde_json::to_vec_pretty(&outcome).map_err(io::Error::other)?,
    )?;
    Ok(outcome)
}

fn attempt(
    plan: &Plan,
    file: &str,
    free: impl Fn(&Path) -> io::Result<u64>,
) -> io::Result<(u64, String)> {
    let recipient: age::x25519::Recipient = plan
        .recipient
        .trim()
        .parse()
        .map_err(|e| io::Error::other(format!("the age recipient does not parse: {e}")))?;
    if let Some(last) = last_archive_size(&plan.target)? {
        let available = free(&plan.target)?;
        if !enough_space(available, last) {
            return Err(io::Error::other(format!(
                "{available} bytes free under {}, less than {SPACE_FACTOR} times the last archive ({last} bytes)",
                plan.target.display()
            )));
        }
    }
    let final_path = plan.target.join(file);
    let partial = plan.target.join(format!(".{file}.partial"));
    let written = write_archive(plan, &recipient, &partial);
    let (bytes, sha256) = match written {
        Ok(v) => v,
        Err(e) => {
            let _ = fs::remove_file(&partial);
            return Err(e);
        }
    };
    fs::rename(&partial, &final_path)?;
    write_atomically(
        &plan.target.join(format!("{file}.sha256")),
        format!("{sha256}  {file}\n").as_bytes(),
    )?;
    Ok((bytes, sha256))
}

fn write_archive(
    plan: &Plan,
    recipient: &age::x25519::Recipient,
    path: &Path,
) -> io::Result<(u64, String)> {
    let out = HashingWriter::new(File::create(path)?);
    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(recipient as &dyn age::Recipient))
            .map_err(io::Error::other)?;
    let encrypted = encryptor.wrap_output(out)?;
    let compressed = zstd::Encoder::new(encrypted, 3)?;
    let mut tar = tar::Builder::new(compressed);
    tar.mode(tar::HeaderMode::Deterministic);
    for source in &plan.sources {
        append_source(&mut tar, source)?;
    }
    let compressed = tar.into_inner()?;
    let encrypted = compressed.finish()?;
    let out = encrypted.finish()?;
    out.file.sync_all()?;
    Ok((out.bytes, hex(&out.hash.finalize())))
}

fn append_source<W: Write>(tar: &mut tar::Builder<W>, source: &Source) -> io::Result<()> {
    if !source.path.exists() {
        return Err(io::Error::other(format!(
            "{} is missing",
            source.path.display()
        )));
    }
    let mut stack = vec![source.path.clone()];
    while let Some(dir) = stack.pop() {
        let mut entries: Vec<_> = fs::read_dir(&dir)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let kind = entry.file_type()?;
            let rel = path.strip_prefix(&source.path).map_err(io::Error::other)?;
            if kind.is_dir() {
                if source.kind == SourceKind::Maildir && entry.file_name() == "tmp" {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            let name = Path::new(&source.name).join(rel);
            let mut file = match File::open(&path) {
                Ok(f) => f,
                // a message moved between the listing and the open
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
            // read up to the length it had when opened: tar writes the size
            // first, and for an AOF still being appended to that length is
            // the consistent prefix
            let len = file.metadata()?.len();
            let mut header = tar::Header::new_gnu();
            header.set_size(len);
            header.set_mode(0o600);
            header.set_cksum();
            tar.append_data(&mut header, &name, (&mut file).take(len))?;
        }
    }
    Ok(())
}

/// The last run's outcome under `target`, if one was recorded.
pub fn last_outcome(target: &Path) -> Option<Outcome> {
    let raw = fs::read(target.join("last.json")).ok()?;
    serde_json::from_slice(&raw).ok()
}

/// Whether `available` bytes leave room for an archive about `last` big.
pub fn enough_space(available: u64, last: u64) -> bool {
    available as f64 >= last as f64 * SPACE_FACTOR
}

/// The size of the newest finished archive under `target`, if any.
fn last_archive_size(target: &Path) -> io::Result<Option<u64>> {
    Ok(archives(target)?.last().map(|(_, size)| *size))
}

/// Finished archives under `target`, oldest first, with their sizes.
fn archives(target: &Path) -> io::Result<Vec<(PathBuf, u64)>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(target)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("mailrs-") && name.ends_with(".tar.age") {
            out.push((entry.path(), entry.metadata()?.len()));
        }
    }
    out.sort();
    Ok(out)
}

/// Keep the newest `keep` archives and their checksums; nothing else under
/// `target` is touched.
fn prune(target: &Path, keep: usize) -> io::Result<()> {
    let all = archives(target)?;
    let drop = all.len().saturating_sub(keep);
    for (path, _) in &all[..drop] {
        let sum = PathBuf::from(format!("{}.sha256", path.display()));
        // the checksum first: a copier takes only archives that have one
        let _ = fs::remove_file(&sum);
        fs::remove_file(path)?;
    }
    Ok(())
}

fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_file_name(format!(
        ".{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    let mut f = File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(&tmp, path)
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A file writer that hashes and counts what passes through.
struct HashingWriter {
    file: File,
    hash: Sha256,
    bytes: u64,
}

impl HashingWriter {
    fn new(file: File) -> Self {
        Self {
            file,
            hash: Sha256::new(),
            bytes: 0,
        }
    }
}

impl Write for HashingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.file.write(buf)?;
        self.hash.update(&buf[..n]);
        self.bytes += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
