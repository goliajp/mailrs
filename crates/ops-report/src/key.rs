//! Per-process report keys.
//!
//! The deployment gives the daemon one master key. Each process's key is
//! `hex(HMAC-SHA256(master, "mailrs-ops-report:" + process))`, computed by
//! `mailrs-ops report-key <process>` and handed to that process alone as
//! `MAILRS_OPS_REPORT_KEY`. The daemon recomputes it from the process name
//! a report carries, so a process holding its own key can only speak as
//! itself.

use crate::wire::PROCESSES;

const LABEL: &str = "mailrs-ops-report:";

/// The report key of `process`, as the hex string the process is given.
/// `None` for a name outside [`PROCESSES`].
pub fn derive(master: &[u8], process: &str) -> Option<String> {
    PROCESSES
        .contains(&process)
        .then(|| mailrs_webhook_signature::sign(master, format!("{LABEL}{process}").as_bytes()))
}

/// The signature header value for `body` under `key` (the hex key string).
pub fn sign(key: &str, body: &[u8]) -> String {
    mailrs_webhook_signature::format_header(&mailrs_webhook_signature::sign(key.as_bytes(), body))
}

/// Whether `header` is the signature of `body` by `process`'s key under
/// `master`. Constant-time in the comparison.
pub fn verify(master: &[u8], process: &str, body: &[u8], header: &str) -> bool {
    derive(master, process)
        .is_some_and(|key| mailrs_webhook_signature::verify_header(key.as_bytes(), body, header))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_process_gets_its_own_key() {
        let a = derive(b"master", "receiver").unwrap();
        let b = derive(b"master", "fastcore").unwrap();
        assert_ne!(a, b);
        assert_eq!(a, derive(b"master", "receiver").unwrap());
        assert_ne!(a, derive(b"other", "receiver").unwrap());
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn a_name_outside_the_set_has_no_key() {
        assert_eq!(derive(b"master", "intruder"), None);
        assert!(!verify(b"master", "intruder", b"x", "sha256=00"));
    }

    #[test]
    fn a_report_signed_by_one_process_does_not_verify_as_another() {
        let receiver = derive(b"m", "receiver").unwrap();
        let sig = sign(&receiver, b"body");
        assert!(verify(b"m", "receiver", b"body", &sig));
        assert!(!verify(b"m", "fastcore", b"body", &sig));
        assert!(!verify(b"m", "receiver", b"other body", &sig));
    }
}
