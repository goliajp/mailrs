use super::*;
use crate::families::screening::{learn_junk_verdict, record_user_junk};
use mailrs_reputation::{NeverList, Set, Source, Stance};

fn newsletter() -> Vec<Vec<u8>> {
    vec![
        b"From: News <news@shop.example>\r\nTo: qa@golia.jp\r\nSubject: picks\r\n\r\nhttps://trk.example/a\r\n"
            .to_vec(),
    ]
}

fn of_source(c: &mut kevy_client::Connection, source: Source) -> Vec<Observation> {
    observations(c)
        .unwrap()
        .into_iter()
        .filter(|o| o.source == source)
        .collect()
}

/// The server put the conversation in Junk, so the reader taking it out
/// has nothing of theirs to retract: before this, nothing was written at
/// all and the correction was lost.
#[test]
fn taking_system_junk_out_records_a_vouch_once() {
    let mut c = kevy_client::Connection::connect("mem://junk-corrections-system").unwrap();
    learn_junk_verdict(&mut c, "qa@golia.jp", "t1", &newsletter(), false, 10);
    let vouches = of_source(&mut c, Source::UserNotJunk);
    assert_eq!(vouches.len(), 1);
    assert_eq!(vouches[0].stance, Stance::Vouch);
    assert_eq!(vouches[0].value, "shop.example");
    assert_eq!(vouches[0].actor.as_deref(), Some("qa@golia.jp"));
    assert_eq!(vouches[0].reason, "took conversation t1 out of Junk");

    learn_junk_verdict(&mut c, "qa@golia.jp", "t1", &newsletter(), false, 11);
    assert_eq!(of_source(&mut c, Source::UserNotJunk).len(), 1, "once");
}

/// The reader had filed it themselves: the correction is the retraction
/// of their marks, and no vouch is added beside it.
#[test]
fn taking_your_own_junk_out_retracts_and_does_not_vouch() {
    let mut c = kevy_client::Connection::connect("mem://junk-corrections-user").unwrap();
    learn_junk_verdict(&mut c, "qa@golia.jp", "t1", &newsletter(), true, 10);
    assert_eq!(
        user_junked_threads(&observations(&mut c).unwrap(), "qa@golia.jp"),
        ["t1".to_string()].into()
    );
    learn_junk_verdict(&mut c, "qa@golia.jp", "t1", &newsletter(), false, 11);
    assert!(of_source(&mut c, Source::UserNotJunk).is_empty());
    let all = observations(&mut c).unwrap();
    assert!(all.iter().any(|o| o.stance == Stance::Retract));
    assert!(user_junked_threads(&all, "qa@golia.jp").is_empty());
}

/// A vouch is evidence, not an allow: it must not lift another reader's
/// junk marks off the domain, or one reader keeping one newsletter would
/// exempt a sender from the rules.
#[test]
fn a_vouch_lifts_nobody_elses_marks() {
    let mut c = kevy_client::Connection::connect("mem://junk-corrections-allow").unwrap();
    let subjects = mailrs_inbound::scan_input::JunkSubjects {
        senders: vec!["shop.example".into()],
        links: Vec::new(),
    };
    record_user_junk(&mut c, "ann@golia.jp", "t9", &subjects, 5).unwrap();
    vouch_not_junk(&mut c, "qa@golia.jp", "t1", &subjects, 6).unwrap();
    let set = Set::build(&observations(&mut c).unwrap(), &NeverList::builtin(&[]), 20);
    let m = mailrs_reputation::Message {
        header_domain: "shop.example".into(),
        ..mailrs_reputation::Message::default()
    };
    assert_eq!(set.facts(&m).sender_marks.unwrap().marks.len(), 1);
}

#[test]
fn junked_threads_are_per_reader() {
    let mut c = kevy_client::Connection::connect("mem://junk-corrections-readers").unwrap();
    learn_junk_verdict(&mut c, "ann@golia.jp", "t2", &newsletter(), true, 10);
    let all = observations(&mut c).unwrap();
    assert!(user_junked_threads(&all, "qa@golia.jp").is_empty());
    assert_eq!(user_junked_threads(&all, "ann@golia.jp").len(), 1);
}
