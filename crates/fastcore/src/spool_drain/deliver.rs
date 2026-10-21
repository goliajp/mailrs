//! The maildir helpers and outbound enqueues the drain shares with the
//! SQL core, under the names the rest of this crate uses.

pub(crate) use mailrs_intake::maildir::{canonical_recipient, deliver, has_maildir};
pub(crate) use mailrs_intake::outbound::{enqueue_outbound, enqueue_redirect};

#[cfg(test)]
mod tests {
    use crate::spool_drain::*;
    use kevy_embedded::{Config, Store};
    use mailrs_core::spool::{SPOOL_SCHEMA_VERSION, SpoolEnvelope, encode_spool_blob};
    use mailrs_mailbox_kevy::KevyMailboxStore;

    fn state() -> Arc<FastcoreState> {
        let store = Arc::new(Store::open(Config::default()).unwrap());
        let mailbox = KevyMailboxStore::new(store);
        Arc::new(FastcoreState::new(mailbox))
    }

    fn envelope(forward_paths: &[&str]) -> SpoolEnvelope {
        SpoolEnvelope {
            reverse_path: "alice@example.com".into(),
            forward_paths: forward_paths.iter().map(|s| s.to_string()).collect(),
            is_authenticated: false,
            conn_id: 1,
            target_folder: "INBOX".into(),
            received_at: 42,
            fraud_verdict: None,
            sandbox_ns: None,
            schema_version: SPOOL_SCHEMA_VERSION,
        }
    }

    fn setup_user_maildir(root: &std::path::Path, addr: &str) -> std::path::PathBuf {
        let (local, domain) = addr.split_once('@').unwrap();
        let base = root.join(domain).join(local);
        for sub in ["cur", "new", "tmp"] {
            std::fs::create_dir_all(base.join(sub)).unwrap();
        }
        base
    }

    #[test]
    fn drain_moves_file_and_strips_envelope() {
        let tmp = tempfile::tempdir().unwrap();
        let spool_new = tmp.path().join("spool").join("incoming").join("new");
        std::fs::create_dir_all(&spool_new).unwrap();
        let maildir_root = tmp.path().join("maildir");
        let user_base = setup_user_maildir(&maildir_root, "bob@example.com");

        let body = b"From: alice@example.com\r\nSubject: hi\r\n\r\nhello\r\n";
        let blob = encode_spool_blob(&envelope(&["bob@example.com"]), body);
        let spool_file = spool_new.join("1000000.M1P1Q1.host");
        std::fs::write(&spool_file, &blob).unwrap();

        let (delivered, _) = drain_once(&spool_new, maildir_root.to_str().unwrap(), &state());
        assert_eq!(delivered, 1);
        assert!(!spool_file.exists());
        let landed = user_base.join("new").join("1000000.M1P1Q1.host");
        assert_eq!(std::fs::read(&landed).unwrap(), body);
    }

    /// An uppercase envelope recipient must still reach its mailbox.
    ///
    /// Reproduces a production file that sat in `.spool/incoming/new`
    /// from 2026-08-06 to 2026-08-14 with
    /// `fwd_paths=["CONTACT@GOLIA.JP"]`. All three resolution attempts
    /// used the address verbatim — the maildir path, the
    /// `mailrs:account:` key and the alias key — and everything on the
    /// other side of them is stored lowercase, so the message was
    /// accepted at SMTP with a 250 and then never delivered. RFC 5321
    /// §2.4 makes the domain case-insensitive outright and urges the
    /// same for the mailbox, which is what the rest of this system
    /// already assumes.
    ///
    /// **This resolves through the kevy account key on purpose, and
    /// deliberately does not pre-create the maildir.** A version that
    /// created `golia.jp/contact/` and relied on the path lookup passes
    /// on macOS whatever the code does, because APFS is case-insensitive
    /// by default — measured: `/tmp/x/GOLIA.JP/CONTACT/new` resolves here
    /// and does not on t02's ext4. Such a test is worse than none: green
    /// on every dev machine, red only in production. kevy keys are bytes,
    /// so this one fails on both until the address is canonicalised.
    #[test]
    fn an_uppercase_recipient_reaches_the_lowercase_mailbox() {
        let tmp = tempfile::tempdir().unwrap();
        let spool_new = tmp.path().join("spool").join("incoming").join("new");
        std::fs::create_dir_all(&spool_new).unwrap();
        let maildir_root = tmp.path().join("maildir");
        std::fs::create_dir_all(&maildir_root).unwrap();

        let st = state();
        st.mailbox
            .upsert_account(
                "contact@golia.jp",
                r#"{"address":"contact@golia.jp","domain":"golia.jp","display_name":"C",
                    "active":true,"created_at":1748000000,"quota_bytes":0}"#,
            )
            .unwrap();

        let body = b"From: alice@example.com\r\nSubject: hi\r\n\r\nhello\r\n";
        let blob = encode_spool_blob(&envelope(&["CONTACT@GOLIA.JP"]), body);
        let spool_file = spool_new.join("1000001.M1P1Q1.host");
        std::fs::write(&spool_file, &blob).unwrap();

        let (delivered, _) = drain_once(&spool_new, maildir_root.to_str().unwrap(), &st);
        assert_eq!(delivered, 1, "uppercase recipient was left in the spool");
        assert!(!spool_file.exists());
        assert_eq!(
            std::fs::read(
                maildir_root
                    .join("golia.jp")
                    .join("contact")
                    .join("new")
                    .join("1000001.M1P1Q1.host")
            )
            .unwrap(),
            body,
            "delivered somewhere other than the canonical mailbox"
        );
    }

    /// Folding an SRS address destroys it, which is why the SRS branch
    /// in `drain_once` reads the raw envelope value and not the folded one.
    ///
    /// `reverse` strips a literal uppercase `SRS0=` prefix, and the HMAC it
    /// checks is computed over the original domain and local part carried
    /// in the address — so folding breaks it twice over. Had the fold
    /// reached that branch, bounces for mail we forwarded would have
    /// stopped being relayed to the original sender and would have gone to
    /// `unresolved` instead, sitting in the spool: the very defect the fold
    /// was added to remove.
    #[test]
    fn folding_an_srs_address_destroys_it() {
        let secret = "srs-test-secret";
        let srs = mailrs_srs::rewrite("Alice@Origin.Example", "fwd.golia.jp", secret);
        let window = mailrs_srs::DEFAULT_TIMESTAMP_WINDOW_DAYS;

        assert_eq!(
            mailrs_srs::reverse(&srs, secret, window).as_deref(),
            Some("Alice@Origin.Example"),
            "the raw form must reverse"
        );
        assert_eq!(
            mailrs_srs::reverse(&canonical_recipient(&srs), secret, window),
            None,
            "if the folded form ever reverses, re-check whether the SRS \
             branch may use it after all"
        );
    }

    #[test]
    fn drain_leaves_file_when_no_recipient_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let spool_new = tmp.path().join("spool").join("incoming").join("new");
        std::fs::create_dir_all(&spool_new).unwrap();
        let maildir_root = tmp.path().join("maildir");
        std::fs::create_dir_all(&maildir_root).unwrap();

        let blob = encode_spool_blob(&envelope(&["ghost@example.com"]), b"body");
        let spool_file = spool_new.join("1000001.M1P1Q1.host");
        std::fs::write(&spool_file, &blob).unwrap();

        let (delivered, _) = drain_once(&spool_new, maildir_root.to_str().unwrap(), &state());
        assert_eq!(delivered, 0);
        assert!(spool_file.exists(), "unresolved file must stay in spool");
    }

    #[test]
    fn drain_delivers_partial_and_removes_when_any_matched() {
        let tmp = tempfile::tempdir().unwrap();
        let spool_new = tmp.path().join("spool").join("incoming").join("new");
        std::fs::create_dir_all(&spool_new).unwrap();
        let maildir_root = tmp.path().join("maildir");
        let bob_base = setup_user_maildir(&maildir_root, "bob@example.com");

        let blob = encode_spool_blob(
            &envelope(&["bob@example.com", "ghost@example.com"]),
            b"body",
        );
        let spool_file = spool_new.join("1000002.M1P1Q1.host");
        std::fs::write(&spool_file, &blob).unwrap();

        let (delivered, _) = drain_once(&spool_new, maildir_root.to_str().unwrap(), &state());
        assert_eq!(delivered, 1);
        assert!(!spool_file.exists());
        assert!(bob_base.join("new").join("1000002.M1P1Q1.host").exists());
    }

    #[test]
    fn drain_skips_undecodable_file() {
        let tmp = tempfile::tempdir().unwrap();
        let spool_new = tmp.path().join("spool").join("incoming").join("new");
        std::fs::create_dir_all(&spool_new).unwrap();
        let maildir_root = tmp.path().join("maildir");
        std::fs::create_dir_all(&maildir_root).unwrap();

        let bogus = spool_new.join("garbage");
        std::fs::write(&bogus, b"not a spool envelope").unwrap();

        let (delivered, _) = drain_once(&spool_new, maildir_root.to_str().unwrap(), &state());
        assert_eq!(delivered, 0);
        assert!(bogus.exists());
    }

    #[test]
    fn drain_returns_zero_on_missing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope").join("incoming").join("new");
        let maildir_root = tmp.path().join("maildir");
        std::fs::create_dir_all(&maildir_root).unwrap();
        assert_eq!(
            drain_once(&missing, maildir_root.to_str().unwrap(), &state()).0,
            0
        );
    }

    #[test]
    fn drain_falls_back_to_alias_when_direct_mailbox_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let spool_new = tmp.path().join("spool").join("incoming").join("new");
        std::fs::create_dir_all(&spool_new).unwrap();
        let maildir_root = tmp.path().join("maildir");
        let alice_base = setup_user_maildir(&maildir_root, "alice@example.com");

        let s = state();
        s.mailbox
            .upsert_alias("contact@example.com", "alice@example.com")
            .unwrap();

        let blob = encode_spool_blob(&envelope(&["contact@example.com"]), b"aliased body");
        let spool_file = spool_new.join("1000003.M1P1Q1.host");
        std::fs::write(&spool_file, &blob).unwrap();

        let (delivered, _) = drain_once(&spool_new, maildir_root.to_str().unwrap(), &s);
        assert_eq!(delivered, 1);
        assert!(!spool_file.exists());
        assert!(
            alice_base.join("new").join("1000003.M1P1Q1.host").exists(),
            "aliased delivery must land in the resolved user's maildir"
        );
    }

    #[test]
    fn srs_roundtrip_reverses_to_original() {
        // forward-rewrite an external sender through our domain, then
        // reverse it back — this is the exact path the redirect MAIL
        // FROM + bounce-return use (G6)
        let secret = "test-secret";
        let rewritten = mailrs_srs::rewrite("bob@remote.example", "golia.jp", secret);
        assert!(rewritten.to_ascii_uppercase().starts_with("SRS0="));
        assert!(rewritten.ends_with("@golia.jp"));
        let back = mailrs_srs::reverse(
            &rewritten,
            secret,
            mailrs_srs::DEFAULT_TIMESTAMP_WINDOW_DAYS,
        );
        assert_eq!(back.as_deref(), Some("bob@remote.example"));
        // wrong secret must fail verification
        assert!(mailrs_srs::reverse(&rewritten, "other", 14).is_none());
    }
}
