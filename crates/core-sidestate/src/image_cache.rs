//! Where prefetched remote images are kept: fastcore writes them, webapi
//! serves them. A directory per user, each image named by its URL's hash,
//! so a hash one user's client asks for never finds another user's copy.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The cache directory, `MAILRS_IMAGE_CACHE_DIR` or `/data/image-cache`.
pub fn root() -> PathBuf {
    PathBuf::from(
        std::env::var("MAILRS_IMAGE_CACHE_DIR").unwrap_or_else(|_| "/data/image-cache".into()),
    )
}

/// `user`'s directory.
pub fn user_dir(root: &Path, user: &str) -> PathBuf {
    root.join(hex(&Sha256::digest(user.to_ascii_lowercase().as_bytes())))
}

/// The name an image of `url` is kept under, and asked for by.
pub fn name(url: &str) -> String {
    hex(&Sha256::digest(url.as_bytes()))
}

/// Where `user`'s copy of `url` lives.
pub fn path(root: &Path, user: &str, url: &str) -> PathBuf {
    user_dir(root, user).join(name(url))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
