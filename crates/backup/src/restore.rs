//! Unpacking an archive into an empty directory, with the private key.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// Decrypt `archive` with the age identity in `identity_file` and unpack
/// it under `into`, which must not exist yet or be empty: a restore never
/// writes over data.
pub fn restore(archive: &Path, identity_file: &Path, into: &Path) -> io::Result<()> {
    if into.exists() && std::fs::read_dir(into)?.next().is_some() {
        return Err(io::Error::other(format!("{} is not empty", into.display())));
    }
    std::fs::create_dir_all(into)?;
    let mut identities = String::new();
    File::open(identity_file)?.read_to_string(&mut identities)?;
    let identity: age::x25519::Identity = identities
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("AGE-SECRET-KEY-"))
        .ok_or_else(|| io::Error::other("no AGE-SECRET-KEY line in the identity file"))?
        .parse()
        .map_err(|e| io::Error::other(format!("the identity does not parse: {e}")))?;
    let decryptor = age::Decryptor::new(File::open(archive)?).map_err(io::Error::other)?;
    let plain = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(io::Error::other)?;
    let mut tar = tar::Archive::new(zstd::Decoder::new(plain)?);
    tar.unpack(into)
}
