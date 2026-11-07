//! Restoring a deleted account before its purge: what both cores check,
//! and what they do once their own record is back.

use std::io;
use std::path::{Path, PathBuf};

use kevy_client::Connection;
use mailrs_core_api::method::admin::{RestoreAccountResponse, RestoreRefusal, RestoreRefused};

use super::{maildir, pending};
use crate::families::outbound;

/// The audit action of a restore.
pub const RESTORED: &str = "account.restore";

/// The delete time a parked maildir's name carries: `<local>.<ts>`.
fn parked_id(dir: &Path) -> String {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.rsplit_once('.')
        .map(|(_, ts)| ts.to_string())
        .unwrap_or_default()
}

fn refused(refused: RestoreRefusal, dirs: &[PathBuf]) -> RestoreRefused {
    RestoreRefused {
        refused,
        parked: dirs.iter().map(|d| parked_id(d)).collect(),
    }
}

/// Whether `address` can be restored, and the parked maildir to move back
/// (`None`: it had none). `live` is whether the core holds an account for
/// it; `parked` names one parked maildir by its delete time.
pub fn check(
    c: &mut Connection,
    root: &Path,
    address: &str,
    live: bool,
    parked: Option<&str>,
) -> io::Result<Result<Option<PathBuf>, RestoreRefused>> {
    let dirs = maildir::parked(root, address)?;
    if live {
        return Ok(Err(refused(RestoreRefusal::Live, &dirs)));
    }
    if pending::deleted_at(c, address)?.is_none() {
        return Ok(Err(refused(RestoreRefusal::NotPending, &dirs)));
    }
    let chosen = match (parked, dirs.len()) {
        (None, 0) => return Ok(Ok(None)),
        (None, 1) => dirs[0].clone(),
        (None, _) => return Ok(Err(refused(RestoreRefusal::SeveralParked, &dirs))),
        (Some(id), _) => match dirs.iter().find(|d| parked_id(d) == id) {
            Some(d) => d.clone(),
            None => return Ok(Err(refused(RestoreRefusal::NoSuchParked, &dirs))),
        },
    };
    if live_dir(root, address).is_some_and(|d| d.exists()) {
        return Ok(Err(refused(RestoreRefusal::MaildirInTheWay, &dirs)));
    }
    Ok(Ok(Some(chosen)))
}

fn live_dir(root: &Path, address: &str) -> Option<PathBuf> {
    let (local, domain) = address.split_once('@')?;
    Some(root.join(domain).join(local))
}

/// With the core's account record back: move `dir` back to where delivery
/// finds it, give back the mail the delete withheld (a send whose time is
/// still ahead waits for it again, one whose time passed is a draft, not
/// sent), write the audit row with both counts, and stop waiting for the
/// purge.
pub fn finish(
    c: &mut Connection,
    root: &Path,
    address: &str,
    dir: Option<&Path>,
    by: &str,
    now: i64,
) -> io::Result<RestoreAccountResponse> {
    let parked = match (dir, live_dir(root, address)) {
        (Some(from), Some(to)) => {
            std::fs::rename(from, &to)?;
            maildir::let_go_of(root, from)?;
            parked_id(from)
        }
        _ => "none".to_string(),
    };
    let uploads = crate::families::uploads::root();
    let back = outbound::release(c, address, now, outbound::Lapsed::Draft(&uploads))?;
    let detail = format!(
        "parked={parked} rescheduled={} drafts={} attachments={}",
        back.rescheduled, back.drafts, back.attachments
    );
    crate::families::audit::append(c, by, RESTORED, address, &detail, now)?;
    pending::clear(c, address)?;
    Ok(RestoreAccountResponse {
        rescheduled: back.rescheduled,
        returned_as_drafts: back.drafts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delete(c: &mut Connection, root: &Path, at: i64) {
        std::fs::create_dir_all(root.join("x.test/a/cur")).unwrap();
        std::fs::write(root.join("x.test/a/cur/1"), b"m").unwrap();
        maildir::park(root, "a@x.test", at).unwrap();
        pending::mark(c, "a@x.test", at).unwrap();
    }

    #[test]
    fn a_restore_needs_a_mark_no_account_and_one_parked_maildir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let mut c = Connection::connect("mem://restore-check").unwrap();
        let refusal = |r: io::Result<Result<Option<PathBuf>, RestoreRefused>>| {
            r.unwrap().err().map(|e| e.refused)
        };
        assert_eq!(
            refusal(check(&mut c, root, "a@x.test", false, None)),
            Some(RestoreRefusal::NotPending)
        );
        delete(&mut c, root, 100);
        assert_eq!(
            refusal(check(&mut c, root, "a@x.test", true, None)),
            Some(RestoreRefusal::Live)
        );
        delete(&mut c, root, 200);
        let several = check(&mut c, root, "a@x.test", false, None)
            .unwrap()
            .unwrap_err();
        assert_eq!(several.refused, RestoreRefusal::SeveralParked);
        assert_eq!(several.parked, ["100", "200"]);
        assert_eq!(
            refusal(check(&mut c, root, "a@x.test", false, Some("300"))),
            Some(RestoreRefusal::NoSuchParked)
        );
        let chosen = check(&mut c, root, "a@x.test", false, Some("200"))
            .unwrap()
            .unwrap();
        assert_eq!(chosen, Some(root.join(".deleted/x.test/a.200")));

        std::fs::create_dir_all(root.join("x.test/a")).unwrap();
        assert_eq!(
            refusal(check(&mut c, root, "a@x.test", false, Some("200"))),
            Some(RestoreRefusal::MaildirInTheWay)
        );
        std::fs::remove_dir(root.join("x.test/a")).unwrap();

        finish(
            &mut c,
            root,
            "a@x.test",
            chosen.as_deref(),
            "admin@x.test",
            300,
        )
        .unwrap();
        assert!(root.join("x.test/a/cur/1").exists());
        assert!(pending::deleted_at(&mut c, "a@x.test").unwrap().is_none());
        assert_eq!(
            maildir::parked(root, "a@x.test").unwrap(),
            [root.join(".deleted/x.test/a.100")]
        );
    }
}
