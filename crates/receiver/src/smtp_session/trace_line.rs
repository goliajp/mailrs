//! What of a client's command line may leave the session.
//!
//! The protocol trace goes to every subscriber of the monitor stream, and
//! `AUTH PLAIN <base64>` carries the username and password. Continuation
//! lines of a SASL exchange are read in `auth.rs` and never reach the trace;
//! the initial response on the command line itself does, so it is cut here.

/// `line` as it may be published: an `AUTH` command keeps its mechanism and
/// loses everything after it.
pub(super) fn traced(line: &str) -> String {
    let mut words = line.split_whitespace();
    let Some(verb) = words.next() else {
        return line.to_string();
    };
    if !verb.eq_ignore_ascii_case("AUTH") {
        return line.to_string();
    }
    match (words.next(), words.next()) {
        (Some(mech), Some(_)) => format!("{verb} {mech} [redacted]"),
        (Some(mech), None) => format!("{verb} {mech}"),
        (None, _) => verb.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::traced;

    #[test]
    fn an_initial_response_never_leaves_the_session() {
        // base64 of "\0user\0secret"
        let line = "AUTH PLAIN AHVzZXIAc2VjcmV0";
        assert_eq!(traced(line), "AUTH PLAIN [redacted]");
        assert_eq!(traced("auth login dXNlcg=="), "auth login [redacted]");
        assert!(!traced("Auth Plain AHVzZXIAc2VjcmV0").contains("AHVz"));
    }

    #[test]
    fn a_mechanism_alone_and_other_commands_are_kept() {
        assert_eq!(traced("AUTH LOGIN"), "AUTH LOGIN");
        assert_eq!(traced("MAIL FROM:<a@b.c>"), "MAIL FROM:<a@b.c>");
        assert_eq!(traced("EHLO mx.example"), "EHLO mx.example");
        // a verb that only starts with AUTH is not AUTH
        assert_eq!(traced("AUTHX PLAIN abc"), "AUTHX PLAIN abc");
    }
}
