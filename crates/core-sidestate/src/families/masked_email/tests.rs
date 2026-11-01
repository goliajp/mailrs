use super::*;

fn conn(name: &str) -> Connection {
    Connection::connect(&format!("mem://masked-email-{name}")).expect("kevy")
}

fn never(_: &str) -> io::Result<bool> {
    Ok(false)
}

/// Picks 0, 1, 2, … so successive candidates differ.
fn counter() -> impl FnMut(usize) -> usize {
    let mut n = 0usize;
    move |max| {
        n += 1;
        n % max
    }
}

fn site(d: &str) -> NewMask {
    NewMask {
        for_domain: d.into(),
        description: "shopping".into(),
        created_by: "web".into(),
        ..NewMask::default()
    }
}

#[test]
fn a_mask_delivers_to_its_owner_and_is_listed() {
    let mut c = conn("deliver");
    let m = create(
        &mut c,
        "Me@X.jp",
        "x.jp",
        site("shop.example"),
        100,
        counter(),
        never,
    )
    .unwrap();
    assert!(m.email.ends_with("@x.jp"));
    assert_eq!(m.owner, "me@x.jp");
    assert_eq!(m.state, State::Enabled);
    assert_eq!(
        delivery(&mut c, &m.email).unwrap(),
        Some(Some("me@x.jp".into()))
    );
    assert_eq!(list(&mut c, "me@x.jp").unwrap(), vec![m.clone()]);
    assert_eq!(sendable(&mut c, "me@x.jp").unwrap(), vec![m.email.clone()]);
    assert_eq!(delivery(&mut c, "nobody@x.jp").unwrap(), None);
}

/// Disabled and deleted take no mail; deleted leaves the list and is never
/// issued again, nor brought back.
#[test]
fn a_deleted_mask_is_a_tombstone_for_ever() {
    let mut c = conn("tombstone");
    let m = create(
        &mut c,
        "me@x.jp",
        "x.jp",
        site("a.example"),
        1,
        counter(),
        never,
    )
    .unwrap();
    let off = Patch {
        state: Some(State::Disabled),
        ..Patch::default()
    };
    update(&mut c, "me@x.jp", &m.email, off).unwrap();
    assert_eq!(delivery(&mut c, &m.email).unwrap(), Some(None));
    assert!(sendable(&mut c, "me@x.jp").unwrap().is_empty());
    let gone = Patch {
        state: Some(State::Deleted),
        ..Patch::default()
    };
    update(&mut c, "me@x.jp", &m.email, gone).unwrap();
    assert_eq!(delivery(&mut c, &m.email).unwrap(), Some(None));
    assert!(list(&mut c, "me@x.jp").unwrap().is_empty());
    assert!(is_taken(&mut c, &m.email).unwrap());
    let back = Patch {
        state: Some(State::Enabled),
        ..Patch::default()
    };
    assert_eq!(
        update(&mut c, "me@x.jp", &m.email, back),
        Err(UpdateError::Deleted)
    );
    // the same draw of words is skipped, for anyone
    let again = create(
        &mut c,
        "other@x.jp",
        "x.jp",
        site("b.example"),
        2,
        counter(),
        never,
    )
    .unwrap();
    assert_ne!(again.email, m.email);
}

#[test]
fn only_the_owner_changes_a_mask() {
    let mut c = conn("owner");
    let m = create(
        &mut c,
        "me@x.jp",
        "x.jp",
        site("a.example"),
        1,
        counter(),
        never,
    )
    .unwrap();
    let p = Patch {
        description: Some("mine".into()),
        ..Patch::default()
    };
    assert_eq!(
        update(&mut c, "you@x.jp", &m.email, p),
        Err(UpdateError::NotFound)
    );
}

#[test]
fn an_address_taken_elsewhere_is_skipped() {
    let mut c = conn("elsewhere");
    let first = candidate("x.jp", counter());
    let m = create(
        &mut c,
        "me@x.jp",
        "x.jp",
        site("a.example"),
        1,
        counter(),
        |a| Ok(a == first),
    )
    .unwrap();
    assert_ne!(m.email, first);
}

#[test]
fn creation_is_limited_per_day() {
    let mut c = conn("daily");
    for _ in 0..MAX_PER_DAY {
        create(&mut c, "me@x.jp", "x.jp", site("a"), 10, os_pick, never).unwrap();
    }
    assert_eq!(
        create(&mut c, "me@x.jp", "x.jp", site("a"), 10, os_pick, never),
        Err(CreateError::TooManyToday)
    );
    // the next UTC day starts again
    create(
        &mut c,
        "me@x.jp",
        "x.jp",
        site("a"),
        10 + 86_400,
        os_pick,
        never,
    )
    .unwrap();
}

#[test]
fn a_pending_mask_is_enabled_by_its_first_message() {
    let mut c = conn("pending");
    let new = NewMask {
        state: Some(State::Pending),
        ..site("a")
    };
    let m = create(&mut c, "me@x.jp", "x.jp", new, 1, counter(), never).unwrap();
    assert_eq!(
        delivery(&mut c, &m.email).unwrap(),
        Some(Some("me@x.jp".into()))
    );
    touch(&mut c, &m.email, 500).unwrap();
    let after = get(&mut c, &m.email).unwrap().unwrap();
    assert_eq!(after.state, State::Enabled);
    assert_eq!(after.last_message_at, Some(500));
}

/// Words an organisation answers mail at, and words nobody wants in their
/// address. None may be drawn.
const DENY: &[&str] = &[
    "abuse",
    "account",
    "accounts",
    "admin",
    "alert",
    "bank",
    "billing",
    "ceo",
    "contact",
    "finance",
    "gov",
    "help",
    "helpdesk",
    "hr",
    "info",
    "invoice",
    "it",
    "legal",
    "login",
    "mail",
    "manager",
    "news",
    "noreply",
    "notice",
    "office",
    "official",
    "password",
    "pay",
    "payment",
    "payroll",
    "police",
    "postmaster",
    "privacy",
    "refund",
    "root",
    "sales",
    "secure",
    "security",
    "service",
    "staff",
    "support",
    "system",
    "tax",
    "team",
    "update",
    "verify",
    "webmaster",
];

#[test]
fn the_words_name_no_role_and_repeat_nothing() {
    for list in [&*words::ADJECTIVES, &*words::NOUNS] {
        let mut seen = std::collections::HashSet::new();
        for w in list {
            assert!(!DENY.contains(w), "{w} is on the deny list");
            assert!(w.chars().all(|c| c.is_ascii_lowercase()), "{w}");
            assert!(seen.insert(*w), "{w} twice");
        }
    }
}
