//! Tests for `mail_auth` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn spf_result_str_covers_all_variants() {
    assert_eq!(spf_result_str(mail_auth::SpfResult::Pass), "pass");
    assert_eq!(spf_result_str(mail_auth::SpfResult::Fail), "fail");
    assert_eq!(spf_result_str(mail_auth::SpfResult::SoftFail), "softfail");
    assert_eq!(spf_result_str(mail_auth::SpfResult::Neutral), "neutral");
    assert_eq!(spf_result_str(mail_auth::SpfResult::None), "none");
    assert_eq!(spf_result_str(mail_auth::SpfResult::TempError), "temperror");
    assert_eq!(spf_result_str(mail_auth::SpfResult::PermError), "permerror");
}
