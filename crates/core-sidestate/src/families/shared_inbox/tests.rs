use kevy_client::Connection;

use super::assign::{self, AssignFact};
use super::notes::{self, Refused};
use super::presence::{self, State};
use super::{ConvRef, conv_for_arrival, copies, drafts, record_copy};

fn conn(name: &str) -> Connection {
    Connection::connect(&format!("mem://shared-inbox-{name}")).unwrap()
}

#[test]
fn a_reply_joins_the_conversation_of_what_it_answers() {
    let mut c = conn("conv");
    let first = conv_for_arrival(&mut c, 7, "<a@x>", "", &[]).unwrap();
    let reply = conv_for_arrival(&mut c, 7, "<b@x>", "<a@x>", &["<a@x>".into()]).unwrap();
    let cited =
        conv_for_arrival(&mut c, 7, "<c@x>", "", &["<A@x>".into(), "<b@x>".into()]).unwrap();
    let other = conv_for_arrival(&mut c, 7, "<d@x>", "", &[]).unwrap();
    assert_eq!(first, reply);
    assert_eq!(first, cited);
    assert_ne!(first, other);
    // delivered twice, the same conversation
    assert_eq!(
        conv_for_arrival(&mut c, 7, "<d@x>", "", &[]).unwrap(),
        other
    );
    // another group numbers its own
    assert_eq!(
        conv_for_arrival(&mut c, 8, "<a@x>", "", &[]).unwrap().gid,
        8
    );
}

#[test]
fn a_tag_reads_back() {
    let r = ConvRef { gid: 3, conv: 41 };
    assert_eq!(ConvRef::parse(&r.tag()), Some(r));
    assert_eq!(ConvRef::parse("3"), None);
    assert_eq!(ConvRef::parse("x:1"), None);
}

#[test]
fn the_current_assignee_is_the_last_fact() {
    let mut c = conn("assign");
    let r = ConvRef { gid: 1, conv: 1 };
    assert_eq!(assign::current(&mut c, r).unwrap(), None);
    let fact = |at, to: Option<&str>| AssignFact {
        at,
        by: "a@x".into(),
        to: to.map(Into::into),
    };
    assign::append(&mut c, r, &fact(1, Some("b@x"))).unwrap();
    assign::append(&mut c, r, &fact(2, Some("a@x"))).unwrap();
    assert_eq!(assign::current(&mut c, r).unwrap().as_deref(), Some("a@x"));
    assign::append(&mut c, r, &fact(3, None)).unwrap();
    assert_eq!(assign::current(&mut c, r).unwrap(), None);
    assert_eq!(assign::history(&mut c, r).unwrap().len(), 3);
}

#[test]
fn notes_fold_edits_and_hide_deletions_but_keep_the_text() {
    let mut c = conn("notes");
    let r = ConvRef { gid: 1, conv: 2 };
    let one = notes::add(&mut c, r, "a@x", "call them back", 10).unwrap();
    let two = notes::add(&mut c, r, "b@x", "refund approved", 11).unwrap();
    assert_eq!(
        notes::edit(&mut c, r, one, "b@x", "x", 12).unwrap(),
        Err(Refused::NotAuthor)
    );
    notes::edit(&mut c, r, one, "A@x", "call them tomorrow", 13)
        .unwrap()
        .unwrap();
    notes::delete(&mut c, r, two, "b@x", 14).unwrap().unwrap();
    assert_eq!(
        notes::delete(&mut c, r, two, "b@x", 15).unwrap(),
        Err(Refused::Missing)
    );
    let live = notes::list(&mut c, r).unwrap();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].text, "call them tomorrow");
    assert_eq!(live[0].edited_at, Some(13));
    assert_eq!(notes::deleted_original(&mut c, r, one).unwrap(), None);
    let gone = notes::deleted_original(&mut c, r, two).unwrap().unwrap();
    assert_eq!(gone.versions, ["refund approved"]);
    assert_eq!((gone.deleted_at, gone.deleted_by.as_str()), (14, "b@x"));
}

#[test]
fn presence_reports_changes_and_who_is_there() {
    let mut c = conn("presence");
    let r = ConvRef { gid: 1, conv: 3 };
    assert!(presence::heartbeat(&mut c, r, "a@x", State::Viewing).unwrap());
    assert!(!presence::heartbeat(&mut c, r, "a@x", State::Viewing).unwrap());
    assert!(presence::heartbeat(&mut c, r, "a@x", State::Composing).unwrap());
    let members = vec!["a@x".to_string(), "b@x".to_string()];
    assert_eq!(
        presence::present(&mut c, r, &members).unwrap(),
        vec![("a@x".to_string(), State::Composing)]
    );
    assert!(presence::leave(&mut c, r, "a@x").unwrap());
    assert!(presence::present(&mut c, r, &members).unwrap().is_empty());
}

#[test]
fn presence_expires_on_its_own() {
    let mut c = conn("presence-ttl");
    let r = ConvRef { gid: 1, conv: 4 };
    presence::heartbeat(&mut c, r, "a@x", State::Viewing).unwrap();
    let key = r.key("presence:a@x");
    let ttl = c.ttl_ms(key.as_bytes()).unwrap();
    assert!(ttl > 50_000 && ttl <= 60_000, "ttl {ttl}");
}

fn save_draft(c: &mut Connection, user: &str, id: i64, body: &str) {
    let d = mailrs_core_api::method::admin::DraftWire {
        id,
        to: "customer@outside.test".into(),
        cc: String::new(),
        bcc: String::new(),
        subject: "Re: order".into(),
        body: body.into(),
        reply_to_thread_id: Some("t-a".into()),
        from: "team@x".into(),
        scheduled_at: None,
        created_at: 1,
        updated_at: 1,
    };
    c.hset(
        crate::families::prefs::drafts_key(user).as_bytes(),
        &[(
            id.to_string().as_bytes(),
            serde_json::to_vec(&d).unwrap().as_slice(),
        )],
    )
    .unwrap();
}

#[test]
fn a_takeover_copies_the_draft_and_locks_out_the_earlier_author() {
    let mut c = conn("drafts");
    let r = ConvRef { gid: 1, conv: 5 };
    record_copy(&mut c, r, "b@x", "t-b").unwrap();
    save_draft(&mut c, "a@x", 1, "half written");
    drafts::announce(&mut c, r, "a@x", 1, 10).unwrap().unwrap();
    // someone else's save does not move it
    assert!(drafts::announce(&mut c, r, "b@x", 9, 11).unwrap().is_err());
    let h = drafts::take_over(&mut c, r, "b@x", 12).unwrap().unwrap();
    assert_eq!((h.from.as_str(), h.to.as_str()), ("a@x", "b@x"));
    let p = drafts::pointer(&mut c, r).unwrap().unwrap();
    assert_eq!((p.author.as_str(), p.draft_id), ("b@x", h.draft_id));
    let copy = drafts::read(&mut c, &p).unwrap().unwrap();
    assert_eq!(copy.body, "half written");
    assert_eq!(copy.reply_to_thread_id.as_deref(), Some("t-b"));
    // the earlier author can no longer move it, nor clear it
    assert!(drafts::announce(&mut c, r, "a@x", 1, 13).unwrap().is_err());
    assert!(!drafts::clear(&mut c, r, "a@x").unwrap());
    assert_eq!(drafts::handoffs(&mut c, r).unwrap(), vec![h]);
    // taking over one's own reply is nothing
    assert_eq!(drafts::take_over(&mut c, r, "b@x", 14).unwrap(), None);
    assert!(drafts::clear(&mut c, r, "b@x").unwrap());
    assert_eq!(drafts::pointer(&mut c, r).unwrap(), None);
}

#[test]
fn copies_list_each_member_once() {
    let mut c = conn("copies");
    let r = ConvRef { gid: 2, conv: 1 };
    record_copy(&mut c, r, "B@x", "t1").unwrap();
    record_copy(&mut c, r, "a@x", "t2").unwrap();
    record_copy(&mut c, r, "b@x", "t3").unwrap();
    assert_eq!(
        copies(&mut c, r).unwrap(),
        vec![("a@x".into(), "t2".into()), ("b@x".into(), "t3".into())]
    );
}

fn team(c: &mut Connection, gid: i64, members: &[&str]) {
    use crate::families::email_groups::{GROUPS_KEY, members_key, set_shared};
    let row = serde_json::json!({ "id": gid, "address": "team@x", "name": "team" });
    c.hset(
        GROUPS_KEY.as_bytes(),
        &[(
            gid.to_string().as_bytes(),
            serde_json::to_vec(&row).unwrap().as_slice(),
        )],
    )
    .unwrap();
    let refs: Vec<&[u8]> = members.iter().map(|m| m.as_bytes()).collect();
    c.sadd(members_key(gid).as_bytes(), &refs).unwrap();
    set_shared(c, gid, true).unwrap();
}

#[test]
fn an_assignment_writes_the_fact_every_copy_and_the_webhook() {
    use crate::families::webhook_outbox;
    use crate::families::webhooks::{NewWebhook, create};
    let mut c = conn("assign-all");
    team(&mut c, 4, &["a@x", "b@x"]);
    let r = ConvRef { gid: 4, conv: 1 };
    record_copy(&mut c, r, "a@x", "ta").unwrap();
    record_copy(&mut c, r, "b@x", "tb").unwrap();
    create(
        &mut c,
        NewWebhook {
            account_address: "b@x".into(),
            url: "https://hooks.example/in".into(),
            event_type: "thread.assigned".into(),
            filter_sender: None,
            filter_thread_id: None,
        },
    )
    .unwrap();
    let mut rows = Vec::new();
    let (who, n) = assign::assign(&mut c, r, "a@x", Some("B@x"), 50, |m, t, a| {
        rows.push((m.to_string(), t.to_string(), a.to_string()));
        Ok(())
    })
    .unwrap();
    assert_eq!((who.as_deref(), n), (Some("b@x"), 2));
    assert_eq!(
        rows,
        vec![
            ("a@x".into(), "ta".into(), "b@x".into()),
            ("b@x".into(), "tb".into(), "b@x".into())
        ]
    );
    assert_eq!(assign::current(&mut c, r).unwrap().as_deref(), Some("b@x"));
    let queued = webhook_outbox::claim_due(&mut c, i64::MAX, 10).unwrap();
    assert_eq!(queued.len(), 1);
    let body: serde_json::Value = serde_json::from_str(&queued[0].payload).unwrap();
    assert_eq!(body["event"], "thread.assigned");
    assert_eq!(body["data"]["assignee"], "b@x");
    assert_eq!(body["data"]["thread_id"], "tb");
    // taking it back writes empty rows
    let (who, _) = assign::assign(&mut c, r, "b@x", None, 51, |_, _, a| {
        assert_eq!(a, "");
        Ok(())
    })
    .unwrap();
    assert_eq!(who, None);
}

#[test]
fn only_members_assign_and_only_to_members() {
    let mut c = conn("assign-refused");
    team(&mut c, 5, &["a@x"]);
    let r = ConvRef { gid: 5, conv: 1 };
    let none = |_: &str, _: &str, _: &str| Ok(());
    assert!(matches!(
        assign::assign(&mut c, r, "admin@x", Some("a@x"), 1, none),
        Err(assign::Refused::NotMember)
    ));
    assert!(matches!(
        assign::assign(&mut c, r, "a@x", Some("out@else"), 1, none),
        Err(assign::Refused::AssigneeNotMember)
    ));
    assert!(assign::history(&mut c, r).unwrap().is_empty());
}
