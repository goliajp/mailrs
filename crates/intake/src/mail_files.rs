//! The one way a message file leaves a user's maildir, in either core: the
//! file goes, and its size comes off the user's used bytes in the network
//! kevy, which the receiver's quota stage reads. Arrivals add their size
//! where they are stored (`effects::before_store`, an IMAP APPEND or COPY);
//! a removal anywhere else would leave the counter high, and an account
//! whose mail only ever goes through the Trash would fill its quota with
//! mail it no longer has.

use std::io;
use std::path::Path;

/// Remove the message file at `path`, of `user`'s mail, its size off
/// the usage on `net`, the core's network kevy. Its size, or `Ok(None)`
/// when there was no such file.
///
/// # Errors
/// The file is there and could not be removed.
pub fn remove_file(
    net: Option<&mut kevy_client::Connection>,
    user: &str,
    path: &Path,
) -> io::Result<Option<u64>> {
    let size = match std::fs::metadata(path) {
        Ok(m) => m.len(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    }
    crate::live_sync::adjust_usage_on(net, user, -(size as i64));
    Ok(Some(size))
}

/// [`remove_file`] for the file `blob_ref` names in `user`'s maildir under
/// `maildir_root`, wherever its flags have moved it.
///
/// # Errors
/// The folder could not be read, or the file could not be removed.
pub fn remove_blob(
    net: Option<&mut kevy_client::Connection>,
    maildir_root: &str,
    user: &str,
    blob_ref: &str,
) -> io::Result<Option<u64>> {
    let Some((local, domain)) = user.split_once('@') else {
        return Ok(None);
    };
    let root = Path::new(maildir_root).join(domain).join(local);
    match mailrs_maildir::find(&root, blob_ref)? {
        Some(path) => remove_file(net, user, &path),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_removed_file_reports_its_size_and_a_missing_one_none() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("x.test").join("u");
        for sub in ["new", "cur", "tmp"] {
            std::fs::create_dir_all(home.join(".Junk").join(sub)).unwrap();
        }
        std::fs::write(home.join(".Junk/cur/1.M1.h:2,S"), b"12345").unwrap();
        let root = tmp.path().to_string_lossy().to_string();
        let mut net = kevy_client::Connection::connect("mem://mail-files-size").unwrap();
        let used = mailrs_core_sidestate::keys::quota::used_key("u@x.test");
        net.set(used.as_bytes(), b"105").unwrap();
        let gone = remove_blob(Some(&mut net), &root, "u@x.test", ".Junk/1.M1.h").unwrap();
        assert_eq!(gone, Some(5));
        assert!(!home.join(".Junk/cur/1.M1.h:2,S").exists());
        assert_eq!(net.get(used.as_bytes()).unwrap(), Some(b"100".to_vec()));
        let again = remove_blob(Some(&mut net), &root, "u@x.test", ".Junk/1.M1.h").unwrap();
        assert_eq!(again, None);
        assert_eq!(net.get(used.as_bytes()).unwrap(), Some(b"100".to_vec()));
    }
}
