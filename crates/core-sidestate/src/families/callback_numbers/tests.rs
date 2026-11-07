use super::*;

fn conn(name: &str) -> Connection {
    Connection::connect(&format!("mem://callnum-{name}")).unwrap()
}

fn nums(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn numbers_of(c: &mut Connection, user: &str, sender: &str) -> Vec<(String, i64, String)> {
    given(c, user, sender)
        .unwrap()
        .into_iter()
        .map(|g| (g.number, g.first_seen, g.first_message_id))
        .collect()
}

#[test]
fn each_number_names_the_earliest_message_that_gave_it() {
    let mut c = conn("given");
    record(
        &mut c,
        "Bob@x.example",
        "Billing@Shop.example",
        &nums(&["0312345678"]),
        200,
        "<b@shop>",
    )
    .unwrap();
    record(
        &mut c,
        "bob@x.example",
        "billing@shop.example",
        &nums(&["0312345678", "0570000123"]),
        100,
        "<a@shop>",
    )
    .unwrap();
    assert_eq!(
        numbers_of(&mut c, "bob@x.example", "billing@shop.example"),
        vec![
            ("0312345678".into(), 100, "<a@shop>".into()),
            ("0570000123".into(), 100, "<a@shop>".into()),
        ]
    );
    assert!(numbers_of(&mut c, "carol@x.example", "billing@shop.example").is_empty());
}

#[test]
fn recording_the_same_message_twice_writes_nothing_the_second_time() {
    let mut c = conn("converge");
    let first = record(&mut c, "u@x", "s@y", &nums(&["0312345678"]), 200, "<m>").unwrap();
    assert_eq!(first, 1);
    let again = record(&mut c, "u@x", "s@y", &nums(&["0312345678"]), 200, "<m>").unwrap();
    assert_eq!(again, 0);
    let later = record(&mut c, "u@x", "s@y", &nums(&["0312345678"]), 300, "<n>").unwrap();
    assert_eq!(
        later, 0,
        "a later message with a known number changes no number"
    );
    assert_eq!(
        numbers_of(&mut c, "u@x", "s@y"),
        vec![("0312345678".into(), 200, "<m>".into())]
    );
}

#[test]
fn a_message_id_with_spaces_reads_back_whole() {
    let mut c = conn("spaces");
    record(&mut c, "u@x", "s@y", &nums(&["0312345678"]), 5, "<a b@c>").unwrap();
    assert_eq!(numbers_of(&mut c, "u@x", "s@y")[0].2, "<a b@c>");
}

/// One message per number, `MAX_PER_SENDER + 8` of them, each at its own
/// time.
fn many() -> Vec<(String, i64, String)> {
    (0..MAX_PER_SENDER as i64 + 8)
        .map(|n| (format!("0{n:010}"), 100 + n, format!("<m{n}>")))
        .collect()
}

#[test]
fn a_full_history_keeps_the_earliest_numbers_whatever_the_order() {
    let all = many();
    let mut forward = conn("full-forward");
    let mut backward = conn("full-backward");
    for (n, at, id) in &all {
        record(&mut forward, "u@x", "s@y", std::slice::from_ref(n), *at, id).unwrap();
    }
    for (n, at, id) in all.iter().rev() {
        record(
            &mut backward,
            "u@x",
            "s@y",
            std::slice::from_ref(n),
            *at,
            id,
        )
        .unwrap();
    }
    let kept = given(&mut forward, "u@x", "s@y").unwrap();
    assert_eq!(kept.len(), MAX_PER_SENDER);
    assert_eq!(kept, given(&mut backward, "u@x", "s@y").unwrap());
    assert_eq!(
        kept.last().unwrap().first_seen,
        100 + MAX_PER_SENDER as i64 - 1
    );
}

/// A walk over the same mail twice, in an order that is not the mail's
/// own (a backfill walks threads, not time): the second walk changes
/// nothing, even for a sender with more numbers than are kept.
#[test]
fn a_second_walk_over_a_full_history_changes_nothing() {
    let mut all = many();
    // later numbers first, then the earlier ones
    all.rotate_left(MAX_PER_SENDER);
    let mut c = conn("full-twice");
    for (n, at, id) in &all {
        record(&mut c, "u@x", "s@y", std::slice::from_ref(n), *at, id).unwrap();
    }
    let mut changed = 0;
    for (n, at, id) in &all {
        changed += record(&mut c, "u@x", "s@y", std::slice::from_ref(n), *at, id).unwrap();
    }
    assert_eq!(changed, 0);
}

#[test]
fn a_history_expires_after_its_latest_message() {
    let mut c = conn("ttl");
    record(&mut c, "u@x", "s@y", &nums(&["0312345678"]), 100, "<m>").unwrap();
    let ttl = c.ttl_ms(key("u@x", "s@y").as_bytes()).unwrap();
    assert!(ttl > 0 && ttl <= (KEEP_DAYS * 86_400_000) as i64, "{ttl}");
}

#[test]
fn nothing_to_record_writes_nothing() {
    let mut c = conn("empty");
    assert_eq!(record(&mut c, "u@x", "s@y", &[], 100, "<m>").unwrap(), 0);
    assert_eq!(c.exists(&[key("u@x", "s@y").as_bytes()]).unwrap(), 0);
}

#[test]
fn numbers_written_two_ways_are_one_number() {
    let mut c = conn("spellings");
    record_numbers(
        &mut c,
        "u@x",
        "Shop <S@Y>",
        &nums(&["03-1234-5678", "03 1234 5678"]),
        100,
        "<m>",
    )
    .unwrap();
    assert_eq!(
        numbers_of(&mut c, "u@x", "s@y"),
        vec![("0312345678".into(), 100, "<m>".into())]
    );
}
