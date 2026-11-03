//! What a Sieve script may do with a user's mail beyond filing it, held to
//! the rules forwarding set on the web is held to. A script reaches the
//! store over ManageSieve with only an app password, so without these it
//! would be the way around them: mail sent on to an address nobody
//! verified, the owner never told, or a vacation reply sent as somebody
//! else.
//!
//! Both cores store scripts through [`store_script`], and the drain checks
//! each redirect again at delivery with [`may_redirect`], so a script
//! stored before these rules, or by an administrator, forwards nowhere it
//! could not have been stored with.

use std::io;

use mailrs_core_sidestate::families::forwarding;
use mailrs_core_sidestate::families::owner_notices::{self, Change, OwnerNotice};

/// Whether `user`'s mail may be redirected to `target`: an address in one
/// of our domains, or the forwarding address the user verified on the web.
///
/// # Errors
/// The forwarding settings could not be read.
pub fn may_redirect(
    conn: &mut kevy_client::Connection,
    user: &str,
    target: &str,
    local_domains: &[String],
) -> io::Result<bool> {
    let target = target.trim().trim_matches(['<', '>']);
    let Some((_, domain)) = target.rsplit_once('@') else {
        return Ok(false);
    };
    if local_domains.iter().any(|d| d.eq_ignore_ascii_case(domain)) {
        return Ok(true);
    }
    let f = forwarding::get(conn, user)?;
    Ok(f.target().is_some_and(|t| t.eq_ignore_ascii_case(target)))
}

/// What storing a script did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stored {
    Stored,
    /// Not stored, and why, in words for the user.
    Refused(String),
}

/// Store `script` as `user`'s, unless it redirects somewhere
/// [`may_redirect`] refuses. A stored script that redirects anywhere queues
/// a notice to the owner, as turning forwarding on does. `via` names where
/// it came from for that notice.
///
/// # Errors
/// The store could not be read or written.
pub fn store_script(
    conn: &mut kevy_client::Connection,
    user: &str,
    script: &str,
    local_domains: &[String],
    via: &str,
    now: i64,
) -> io::Result<Stored> {
    let reach = match mailrs_sieve::reach(script) {
        Ok(r) => r,
        Err(e) => return Ok(Stored::Refused(e)),
    };
    for target in &reach.redirects {
        if !may_redirect(conn, user, target, local_domains)? {
            return Ok(Stored::Refused(format!(
                "redirect to {target} refused: mail goes outside our domains only to the \
                 forwarding address verified in Settings"
            )));
        }
    }
    let key = mailrs_core_sidestate::sieve_key(user);
    conn.set(key.as_bytes(), script.as_bytes())?;
    if !reach.redirects.is_empty() {
        owner_notices::push(
            conn,
            &OwnerNotice {
                user: user.to_lowercase(),
                change: Change::SieveRedirects {
                    targets: reach.redirects,
                },
                via: via.to_string(),
                at: now,
            },
        )?;
    }
    Ok(Stored::Stored)
}

/// The stored scripts as these rules see them, for an operator deciding
/// whether the rules change what anybody's mail does. Counts and domains
/// only: no script text.
#[derive(Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Survey {
    pub accounts: usize,
    pub scripts: usize,
    /// Scripts that do not parse, which deliver as if absent.
    pub unparsed: usize,
    /// One per account and target domain out of our domains.
    pub external_redirects: Vec<ExternalRedirect>,
    /// Scripts with a vacation `:from` other than the account's address.
    pub foreign_vacation_from: usize,
    /// Of those, the ones whose `:from` the account may not send as, which
    /// now reply as the account.
    pub foreign_vacation_from_refused: usize,
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct ExternalRedirect {
    pub account: String,
    pub target_domain: String,
    /// Every target in the domain is the account's verified forwarding
    /// address, so the script keeps forwarding there.
    pub verified: bool,
}

/// Read every account's script and say what the rules make of it.
///
/// # Errors
/// The store could not be read.
pub fn survey(
    conn: &mut kevy_client::Connection,
    accounts: &[String],
    local_domains: &[String],
    may_send_as: impl Fn(&str, &str) -> bool,
) -> io::Result<Survey> {
    let mut out = Survey {
        accounts: accounts.len(),
        ..Survey::default()
    };
    for user in accounts {
        let key = mailrs_core_sidestate::sieve_key(user);
        let Some(raw) = conn.get(key.as_bytes())? else {
            continue;
        };
        let script = String::from_utf8_lossy(&raw);
        if script.trim().is_empty() {
            continue;
        }
        out.scripts += 1;
        let Ok(reach) = mailrs_sieve::reach(&script) else {
            out.unparsed += 1;
            continue;
        };
        for target in &reach.redirects {
            let domain = target.rsplit_once('@').map_or("", |(_, d)| d);
            let ours = local_domains.iter().any(|d| d.eq_ignore_ascii_case(domain));
            if ours {
                continue;
            }
            let verified = may_redirect(conn, user, target, local_domains)?;
            let seen = out
                .external_redirects
                .iter_mut()
                .find(|e| e.account == *user && e.target_domain.eq_ignore_ascii_case(domain));
            match seen {
                Some(e) => e.verified &= verified,
                None => out.external_redirects.push(ExternalRedirect {
                    account: user.clone(),
                    target_domain: domain.to_ascii_lowercase(),
                    verified,
                }),
            }
        }
        let foreign: Vec<&String> = reach
            .vacation_froms
            .iter()
            .filter(|f| !f.eq_ignore_ascii_case(user))
            .collect();
        if !foreign.is_empty() {
            out.foreign_vacation_from += 1;
            if foreign.iter().any(|f| !may_send_as(user, f)) {
                out.foreign_vacation_from_refused += 1;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(name: &str) -> kevy_client::Connection {
        kevy_client::Connection::connect(&format!("mem://sieve-policy-{name}")).unwrap()
    }

    fn ours() -> Vec<String> {
        vec!["x.test".to_string()]
    }

    fn verify(c: &mut kevy_client::Connection, user: &str, address: &str) {
        let code = forwarding::request(c, user, address, true, 100).unwrap();
        forwarding::verify(c, user, &code, 101).unwrap();
    }

    #[test]
    fn a_redirect_out_needs_the_verified_forwarding_address() {
        let mut c = conn("verified");
        assert!(may_redirect(&mut c, "u@x.test", "Team@X.test", &ours()).unwrap());
        assert!(!may_redirect(&mut c, "u@x.test", "me@out.test", &ours()).unwrap());
        assert!(!may_redirect(&mut c, "u@x.test", "not an address", &ours()).unwrap());
        verify(&mut c, "u@x.test", "me@out.test");
        assert!(may_redirect(&mut c, "u@x.test", "<ME@out.test>", &ours()).unwrap());
        assert!(!may_redirect(&mut c, "u@x.test", "other@out.test", &ours()).unwrap());
        assert!(
            !may_redirect(&mut c, "v@x.test", "me@out.test", &ours()).unwrap(),
            "another user's verified address is not this one's"
        );
    }

    #[test]
    fn a_script_redirecting_out_unverified_is_not_stored() {
        let mut c = conn("refused");
        let script = r#"if header :contains "subject" "x" { redirect "thief@out.test"; }"#;
        let got = store_script(&mut c, "u@x.test", script, &ours(), "managesieve", 5).unwrap();
        assert!(matches!(got, Stored::Refused(ref why) if why.contains("thief@out.test")));
        let key = mailrs_core_sidestate::sieve_key("u@x.test");
        assert_eq!(c.get(key.as_bytes()).unwrap(), None);
        assert!(owner_notices::take(&mut c, 10).unwrap().is_empty());
    }

    #[test]
    fn a_stored_script_that_redirects_tells_the_owner() {
        let mut c = conn("notice");
        verify(&mut c, "u@x.test", "me@out.test");
        let script = r#"redirect "me@out.test"; redirect "team@x.test";"#;
        let got = store_script(&mut c, "U@x.test", script, &ours(), "managesieve", 5).unwrap();
        assert_eq!(got, Stored::Stored);
        let notices = owner_notices::take(&mut c, 10).unwrap();
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].user, "u@x.test");
        assert_eq!(
            notices[0].change,
            Change::SieveRedirects {
                targets: vec!["me@out.test".into(), "team@x.test".into()]
            }
        );
        // filing only: stored, nobody told
        store_script(&mut c, "u@x.test", "keep;", &ours(), "managesieve", 6).unwrap();
        assert!(owner_notices::take(&mut c, 10).unwrap().is_empty());
    }

    #[test]
    fn a_survey_names_accounts_and_domains_but_no_script() {
        let mut c = conn("survey");
        let put = |c: &mut kevy_client::Connection, user: &str, s: &str| {
            let key = mailrs_core_sidestate::sieve_key(user);
            c.set(key.as_bytes(), s.as_bytes()).unwrap();
        };
        put(
            &mut c,
            "a@x.test",
            r#"redirect "a@out.test"; redirect "b@OUT.test";"#,
        );
        put(&mut c, "b@x.test", r#"redirect "team@x.test";"#);
        put(
            &mut c,
            "c@x.test",
            r#"require "vacation"; vacation :from "boss@x.test" "away";"#,
        );
        put(
            &mut c,
            "d@x.test",
            r#"require "vacation"; vacation :from "team@x.test" "away";"#,
        );
        put(&mut c, "e@x.test", "redirect {");
        verify(&mut c, "a@x.test", "a@out.test");
        let accounts: Vec<String> = ["a", "b", "c", "d", "e", "f"]
            .iter()
            .map(|u| format!("{u}@x.test"))
            .collect();
        let may = |user: &str, from: &str| user == "d@x.test" && from == "team@x.test";
        let s = survey(&mut c, &accounts, &ours(), may).unwrap();
        assert_eq!((s.accounts, s.scripts, s.unparsed), (6, 5, 1));
        assert_eq!(
            s.external_redirects,
            [ExternalRedirect {
                account: "a@x.test".into(),
                target_domain: "out.test".into(),
                verified: false,
            }],
            "one row per account and domain, verified only when every target is"
        );
        assert_eq!(
            (s.foreign_vacation_from, s.foreign_vacation_from_refused),
            (2, 1)
        );
    }
}
