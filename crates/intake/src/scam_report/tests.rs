//! The report flow against an in-memory core and a memory-backed shared
//! store: each ground, each reason to wait, one person counted once, a
//! later report settling an earlier one, an administrator's decision, and
//! twenty copies across five inboxes taken back well inside five minutes.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;

use kevy_client::Connection;
use mailrs_core_sidestate::families::reports;
use mailrs_report::Template;

use super::*;

#[derive(Default)]
struct Mail {
    /// (user, mid) → (tid, held)
    copies: BTreeMap<(String, String), (String, bool)>,
    raw: BTreeMap<String, Vec<u8>>,
    templates: BTreeMap<String, (Template, i64)>,
    held_by_rule: Vec<String>,
    batches: BTreeMap<String, Vec<(String, String)>>,
    /// (user, tid) → when a person last released it
    released: BTreeMap<(String, String), i64>,
}

struct Fake {
    url: String,
    mail: RefCell<Mail>,
    /// A `mem://` store lives while a connection to it is open.
    _keep: Connection,
}

fn fake(url: &str) -> Fake {
    Fake {
        url: url.to_string(),
        mail: RefCell::new(Mail::default()),
        _keep: Connection::connect(url).unwrap(),
    }
}

impl ReportStore for Fake {
    fn net_conn(&self) -> Option<Connection> {
        Connection::connect(&self.url).ok()
    }
    fn conversation_of(&self, user: &str, mid: &str) -> io::Result<Option<(String, bool)>> {
        Ok(self
            .mail
            .borrow()
            .copies
            .get(&(user.into(), mid.into()))
            .cloned())
    }
    fn raw_of(&self, _user: &str, mid: &str) -> Option<Vec<u8>> {
        self.mail.borrow().raw.get(mid).cloned()
    }
    fn stored_template(&self, mid: &str) -> io::Result<Option<Template>> {
        Ok(self
            .mail
            .borrow()
            .templates
            .get(mid)
            .map(|(t, _)| t.clone()))
    }
    fn record_template(&self, mid: &str, t: &Template, at: i64) -> io::Result<bool> {
        let mut m = self.mail.borrow_mut();
        if m.templates.contains_key(mid) {
            return Ok(false);
        }
        m.templates.insert(mid.into(), (t.clone(), at));
        Ok(true)
    }
    fn candidates(&self, simhash: u64, since: i64) -> io::Result<Vec<String>> {
        let pieces = mailrs_report::fingerprint::segments(simhash);
        Ok(self
            .mail
            .borrow()
            .templates
            .iter()
            .filter(|(_, (t, at))| {
                *at >= since
                    && mailrs_report::fingerprint::segments(t.simhash)
                        .iter()
                        .zip(pieces)
                        .any(|(a, b)| *a == b)
            })
            .map(|(m, _)| m.clone())
            .collect())
    }
    fn readers(&self, mid: &str) -> io::Result<Vec<String>> {
        Ok(self
            .mail
            .borrow()
            .copies
            .keys()
            .filter(|(_, m)| m == mid)
            .map(|(u, _)| u.clone())
            .collect())
    }
    fn held_by_rule(&self, mid: &str) -> bool {
        self.mail.borrow().held_by_rule.iter().any(|m| m == mid)
    }
    fn hold(&self, user: &str, tid: &str) -> io::Result<()> {
        for ((u, _), (t, held)) in self.mail.borrow_mut().copies.iter_mut() {
            if u == user && t == tid {
                *held = true;
            }
        }
        Ok(())
    }
    fn take_back(&self, user: &str, tid: &str, mid: &str, batch: &str) -> Result<bool, String> {
        let mut m = self.mail.borrow_mut();
        // the cores' rule: a person's release stands against mail that
        // arrived before it
        let arrived = m.templates.get(mid).map(|(_, at)| *at);
        if let Some(released) = m.released.get(&(user.to_string(), tid.to_string()))
            && arrived.is_none_or(|a| a <= *released)
        {
            return Ok(false);
        }
        let mut taken = false;
        for ((u, _), (t, held)) in m.copies.iter_mut() {
            if u == user && t == tid && !*held {
                *held = true;
                taken = true;
            }
        }
        if taken {
            m.batches
                .entry(batch.into())
                .or_default()
                .push((user.into(), tid.into()));
        }
        Ok(taken)
    }
}

fn scam(name: &str, addr: &str, link: &str) -> Vec<u8> {
    format!(
        "From: Parcel Desk <notice@parcel-desk.example>\r\nTo: {name} <{addr}>\r\n\
         Subject: Your parcel is held\r\nDate: Tue, 3 Nov 2026 10:00:00 +0000\r\n\r\n\
         Dear {name}, your parcel could not be delivered because a customs fee is unpaid. \
         Pay the fee at https://{link}/pay?to={addr} within 24 hours, otherwise the parcel \
         is returned to the sender and the fee doubles.\r\n"
    )
    .into_bytes()
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// `n` readers with their own copy of the same template (message `m<i>`),
/// fingerprinted as delivery would.
fn world(test: &str, n: usize) -> Fake {
    let f = fake(&format!("mem://scam-report-{test}"));
    for i in 0..n {
        let user = format!("u{i}@example.com");
        let mid = format!("m{i}");
        let raw = scam(&format!("Person{i}"), &user, "parcel-desk.example");
        let t = template::template_of(&user, &raw).expect("a fingerprint");
        f.record_template(&mid, &t, now()).unwrap();
        let mut m = f.mail.borrow_mut();
        m.raw.insert(mid.clone(), raw);
        m.copies
            .insert((user, mid.clone()), (format!("t{i}"), false));
    }
    f
}

fn held(f: &Fake, user: &str) -> bool {
    f.mail
        .borrow()
        .copies
        .iter()
        .any(|((u, _), (_, h))| u == user && *h)
}

#[test]
fn one_person_holds_their_own_copy_and_the_rest_wait_for_an_administrator() {
    let f = world("alone", 3);
    let (r, o) = report(&f, "u0@example.com", "m0", false).unwrap().unwrap();
    assert!(held(&f, "u0@example.com"));
    assert!(!held(&f, "u1@example.com") && !held(&f, "u2@example.com"));
    assert_eq!(o.state, "waiting");
    assert_eq!(o.waits, vec!["no_ground".to_string()]);
    assert_eq!(o.inboxes, 2);
    let mut c = f.net_conn().unwrap();
    assert_eq!(reports::pending(&mut c).unwrap(), vec![r.id.clone()]);
    let listed = pending(&f).unwrap();
    assert_eq!(listed[0].samples.len(), 2);
    assert_eq!(listed[0].subject, "Your parcel is held");
}

#[test]
fn an_administrators_report_takes_every_other_copy_back() {
    let f = world("admin", 3);
    let (r, o) = report(&f, "u0@example.com", "m0", true).unwrap().unwrap();
    assert_eq!(o.state, "taken_back");
    assert_eq!(o.grounds, vec!["administrator".to_string()]);
    assert_eq!(o.taken_back, 2);
    assert!(held(&f, "u1@example.com") && held(&f, "u2@example.com"));
    let batch = format!("report:{}", r.id);
    assert_eq!(f.mail.borrow().batches[&batch].len(), 2);
}

#[test]
fn a_hold_rule_on_the_reported_message_is_a_ground() {
    let f = world("rule", 2);
    f.mail.borrow_mut().held_by_rule.push("m0".into());
    let (_, o) = report(&f, "u0@example.com", "m0", false).unwrap().unwrap();
    assert_eq!(
        (o.state.as_str(), o.grounds.as_slice()),
        ("taken_back", &["hold_rule".to_string()][..])
    );
}

#[test]
fn a_second_person_takes_it_back_and_settles_the_first_report() {
    let f = world("two", 4);
    let (first, o1) = report(&f, "u0@example.com", "m0", false).unwrap().unwrap();
    assert_eq!(o1.state, "waiting");
    let (_, o2) = report(&f, "u1@example.com", "m1", false).unwrap().unwrap();
    assert_eq!(o2.state, "taken_back");
    assert_eq!(o2.grounds, vec!["two_people".to_string()]);
    assert!(held(&f, "u2@example.com") && held(&f, "u3@example.com"));
    let mut c = f.net_conn().unwrap();
    let settled = reports::outcome(&mut c, &first.id).unwrap().unwrap();
    assert_eq!(settled.state, "taken_back");
    assert!(reports::pending(&mut c).unwrap().is_empty());
}

#[test]
fn an_account_and_the_account_it_forwards_to_are_one_person() {
    let f = world("forward", 4);
    let mut c = f.net_conn().unwrap();
    mailrs_core_sidestate::families::local_forward::set_targets(
        &mut c,
        "u0@example.com",
        &["u1@example.com".to_string()],
    )
    .unwrap();
    report(&f, "u0@example.com", "m0", false).unwrap();
    let (_, o) = report(&f, "u1@example.com", "m1", false).unwrap().unwrap();
    assert_eq!(o.state, "waiting", "{o:?}");
    assert!(!held(&f, "u2@example.com"));
}

#[test]
fn more_than_twenty_inboxes_waits_even_for_an_administrator() {
    let f = world("many", 22);
    let (_, o) = report(&f, "u0@example.com", "m0", true).unwrap().unwrap();
    assert_eq!(o.state, "waiting");
    assert_eq!(o.waits, vec!["many_inboxes".to_string()]);
    assert_eq!(o.inboxes, 21);
    assert!(!held(&f, "u5@example.com"));
}

#[test]
fn a_sender_known_for_over_ninety_days_waits() {
    let f = world("history", 3);
    let mut c = f.net_conn().unwrap();
    let long_ago = now() - 120 * 86_400;
    mailrs_core_sidestate::families::domain_first::record(
        &mut c,
        &["parcel-desk.example".to_string()],
        long_ago,
    )
    .unwrap();
    let (_, o) = report(&f, "u0@example.com", "m0", true).unwrap().unwrap();
    assert_eq!(o.waits, vec!["long_history".to_string()]);
}

#[test]
fn an_allow_listed_sender_waits() {
    let f = world("allowed", 3);
    let mut c = f.net_conn().unwrap();
    let entry = serde_json::json!({ "value": "parcel-desk.example", "list": "white" }).to_string();
    c.hset(
        mailrs_core_sidestate::keys::admin::GREYLIST_LOCAL_LISTS,
        &[(b"1".as_slice(), entry.as_bytes())],
    )
    .unwrap();
    let (_, o) = report(&f, "u0@example.com", "m0", true).unwrap().unwrap();
    assert_eq!(o.waits, vec!["allow_listed".to_string()]);
}

#[test]
fn no_other_copy_is_none_found() {
    let f = world("lonely", 1);
    let (_, o) = report(&f, "u0@example.com", "m0", true).unwrap().unwrap();
    assert_eq!(o.state, "none_found");
    assert!(held(&f, "u0@example.com"));
}

#[test]
fn a_different_message_is_not_taken_with_the_template() {
    let f = world("other", 2);
    let raw =
        b"From: Board <board@company.example>\r\nTo: u9@example.com\r\nSubject: meeting\r\n\r\n\
        The quarterly board meeting moves to Thursday afternoon. Please bring the revised budget, \
        the hiring plan and your notes on the vendor contract so we can close them.\r\n"
            .to_vec();
    let t = template::template_of("u9@example.com", &raw).unwrap();
    f.record_template("other", &t, now()).unwrap();
    f.mail.borrow_mut().copies.insert(
        ("u9@example.com".into(), "other".into()),
        ("t9".into(), false),
    );
    report(&f, "u0@example.com", "m0", true).unwrap();
    assert!(!held(&f, "u9@example.com"));
    assert!(held(&f, "u1@example.com"));
}

#[test]
fn an_administrator_confirms_or_dismisses_what_waits() {
    let f = world("decide", 3);
    let (r, _) = report(&f, "u0@example.com", "m0", false).unwrap().unwrap();
    let (_, o) = confirm(&f, &r.id, "admin@example.com").unwrap().unwrap();
    assert_eq!((o.state.as_str(), o.taken_back), ("taken_back", 2));
    // a second decision on a settled report changes nothing
    let (_, again) = dismiss(&f, &r.id, "admin@example.com").unwrap().unwrap();
    assert_eq!(again.state, "taken_back");

    let g = world("dismiss", 3);
    let (r, _) = report(&g, "u0@example.com", "m0", false).unwrap().unwrap();
    let (_, o) = dismiss(&g, &r.id, "admin@example.com").unwrap().unwrap();
    assert_eq!(o.state, "dismissed");
    assert!(!held(&g, "u1@example.com"));
    assert!(confirm(&g, "rp-missing", "a").unwrap().is_none());
}

#[test]
fn only_the_reporter_reads_the_outcome() {
    let f = world("mine", 2);
    let (r, _) = report(&f, "u0@example.com", "m0", false).unwrap().unwrap();
    assert!(outcome_for(&f, "u0@example.com", &r.id).unwrap().is_some());
    assert!(outcome_for(&f, "u1@example.com", &r.id).unwrap().is_none());
}

#[test]
fn a_message_the_reporter_does_not_have_is_not_reported() {
    let f = world("absent", 2);
    assert!(report(&f, "u0@example.com", "m1", true).unwrap().is_none());
}

#[test]
fn a_conversation_put_back_is_taken_again_only_for_a_copy_that_came_after() {
    let f = world("put-back", 2);
    let now = now();
    {
        let mut m = f.mail.borrow_mut();
        // u1's copy arrived an hour ago; u1 put the conversation back since
        let (t, _) = m.templates["m1"].clone();
        m.templates.insert("m1".into(), (t, now - 3600));
        m.released
            .insert(("u1@example.com".into(), "t1".into()), now - 600);
    }
    let (_, o) = report(&f, "u0@example.com", "m0", true).unwrap().unwrap();
    assert_eq!((o.state.as_str(), o.taken_back), ("taken_back", 0), "{o:?}");
    assert!(
        !held(&f, "u1@example.com"),
        "a copy from before the release took it back"
    );

    // a second copy of the template lands in the same conversation, after
    // the release: that one may take it
    let raw = scam("Person1", "u1@example.com", "parcel-desk.example");
    let t = template::template_of("u1@example.com", &raw).unwrap();
    f.record_template("m1-later", &t, now).unwrap();
    {
        let mut m = f.mail.borrow_mut();
        m.raw.insert("m1-later".into(), raw);
        m.copies.insert(
            ("u1@example.com".into(), "m1-later".into()),
            ("t1".into(), false),
        );
    }
    let (_, o) = report(&f, "u0@example.com", "m0", true).unwrap().unwrap();
    assert_eq!(o.taken_back, 1, "{o:?}");
    assert!(held(&f, "u1@example.com"));
}

/// The audit rows written so far, newest first.
fn audit_rows(f: &Fake) -> Vec<mailrs_core_sidestate::families::audit::AuditFact> {
    let mut c = f.net_conn().unwrap();
    c.lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .unwrap()
        .iter()
        .filter_map(|r| serde_json::from_slice(r).ok())
        .collect()
}

#[test]
fn the_reporters_other_copies_of_the_template_are_held_too() {
    let f = fake("mem://scam-report-own-copies");
    let me = "qa@example.com";
    for i in 0..4 {
        let mid = format!("q{i}");
        let raw = scam("QA", me, "parcel-desk.example");
        let t = template::template_of(me, &raw).unwrap();
        f.record_template(&mid, &t, now()).unwrap();
        let mut m = f.mail.borrow_mut();
        m.raw.insert(mid.clone(), raw);
        m.copies.insert((me.into(), mid), (format!("qt{i}"), false));
    }
    let (_, o) = report(&f, me, "q0", false).unwrap().unwrap();
    assert_eq!(o.own_held, 3, "{o:?}");
    let held: Vec<bool> = f.mail.borrow().copies.values().map(|(_, h)| *h).collect();
    assert_eq!(held, vec![true; 4], "only the reported copy went to Review");
    let own: Vec<_> = audit_rows(&f)
        .into_iter()
        .filter(|a| a.action == "quarantine.hold" && a.detail.contains("source=scam_report"))
        .collect();
    assert_eq!(own.len(), 4, "{own:?}");
    assert!(own.iter().all(|a| a.actor == me), "{own:?}");
}

#[test]
fn a_report_of_a_body_too_short_to_compare_is_counted() {
    let f = fake("mem://scam-report-short");
    let raw =
        b"From: x@evil.example\r\nTo: u@example.com\r\nSubject: hi\r\n\r\nPay now.\r\n".to_vec();
    {
        let mut m = f.mail.borrow_mut();
        m.raw.insert("short".into(), raw);
        m.copies.insert(
            ("u@example.com".into(), "short".into()),
            ("ts".into(), false),
        );
    }
    let (_, o) = report(&f, "u@example.com", "short", false)
        .unwrap()
        .unwrap();
    assert!(o.no_fingerprint, "{o:?}");
    assert_eq!(o.state, "none_found");
    let mut c = f.net_conn().unwrap();
    let rule = vec![reports::NO_FINGERPRINT.to_string()];
    let n = mailrs_core_sidestate::families::fraud_hits::counts(&mut c, &rule, now()).unwrap();
    assert_eq!(n.today.get(reports::NO_FINGERPRINT), Some(&1));
}

#[test]
fn twenty_copies_in_five_inboxes_are_taken_back_well_inside_five_minutes() {
    let f = fake("mem://scam-report-timing");
    for i in 0..20 {
        let user = format!("u{}@example.com", i % 5);
        let mid = format!("m{i}");
        let raw = scam(&format!("Person{i}"), &user, "parcel-desk.example");
        let t = template::template_of(&user, &raw).unwrap();
        f.record_template(&mid, &t, now()).unwrap();
        let mut m = f.mail.borrow_mut();
        m.raw.insert(mid.clone(), raw);
        m.copies.insert((user, mid), (format!("t{i}"), false));
    }
    let started = std::time::Instant::now();
    let (_, o) = report(&f, "u0@example.com", "m0", true).unwrap().unwrap();
    let took = started.elapsed();
    assert_eq!(o.state, "taken_back");
    assert_eq!(o.inboxes, 4);
    // u0's other three copies are other conversations of the reporter, not
    // other inboxes: the reporter holds only the one reported
    assert_eq!(o.taken_back, 16);
    assert!(took < std::time::Duration::from_secs(300), "{took:?}");
}
