use super::*;

const T: Timing = Timing {
    for_secs: 300,
    clear_secs: 600,
    unknown_wait: 600,
    base: Severity::Warning,
};

fn daily(_: Severity) -> Option<i64> {
    Some(86_400)
}

/// Run one evaluation per minute from `at`, returning the notices.
fn run(st: &mut AlertState, at: &mut i64, evals: &[Eval], t: &Timing) -> Vec<NoticeKind> {
    let mut out = Vec::new();
    for e in evals {
        out.extend(step(st, e, *at, t, daily).notices);
        *at += 60;
    }
    out
}

fn holds() -> Eval {
    Eval::holds(1, "> 0")
}

fn clear() -> Eval {
    Eval::clear(0)
}

fn down() -> Eval {
    Eval::down("refused")
}

fn missing() -> Eval {
    Eval::field("null")
}

#[test]
fn it_fires_once_the_condition_held_for_the_window() {
    let (mut st, mut at) = (AlertState::new(0), 0);
    // pending at 0, 60 .. 240 counted; at 300 it has held 300 s
    let n = run(
        &mut st,
        &mut at,
        &[holds(), holds(), holds(), holds(), holds()],
        &T,
    );
    assert!(n.is_empty());
    assert_eq!(st.phase, Phase::Pending);
    let n = run(&mut st, &mut at, &[holds()], &T);
    assert_eq!(n, [NoticeKind::Firing]);
    assert_eq!(st.phase, Phase::Firing);
}

#[test]
fn a_condition_that_clears_inside_the_window_is_never_sent() {
    let (mut st, mut at) = (AlertState::new(0), 0);
    let n = run(
        &mut st,
        &mut at,
        &[holds(), holds(), clear(), holds(), clear()],
        &T,
    );
    assert!(n.is_empty());
    assert_eq!(st.phase, Phase::Ok);
}

#[test]
fn recovery_waits_for_the_clear_window_and_flapping_back_is_silent() {
    let t = Timing { for_secs: 0, ..T };
    let (mut st, mut at) = (AlertState::new(0), 0);
    assert_eq!(run(&mut st, &mut at, &[holds()], &t), [NoticeKind::Firing]);
    let n = run(&mut st, &mut at, &[clear(), clear(), holds()], &t);
    assert!(n.is_empty(), "back to firing inside clear: {n:?}");
    assert_eq!(st.phase, Phase::Firing);
    let n = run(&mut st, &mut at, &vec![clear(); 11], &t);
    assert_eq!(n, [NoticeKind::Resolved]);
    assert_eq!(st.phase, Phase::Ok);
}

#[test]
fn zero_windows_move_at_once() {
    let t = Timing {
        for_secs: 0,
        clear_secs: 0,
        ..T
    };
    let (mut st, mut at) = (AlertState::new(0), 0);
    assert_eq!(
        run(&mut st, &mut at, &[holds(), clear()], &t),
        [NoticeKind::Firing, NoticeKind::Resolved]
    );
}

#[test]
fn unreadable_is_not_a_change_of_the_condition() {
    let t = Timing { for_secs: 0, ..T };
    let (mut st, mut at) = (AlertState::new(0), 0);
    assert_eq!(run(&mut st, &mut at, &[holds()], &t), [NoticeKind::Firing]);
    // three misses make it unknown, but neither "resolved" nor a new firing
    let n = run(&mut st, &mut at, &[missing(), missing(), missing()], &t);
    assert!(n.is_empty(), "{n:?}");
    assert_eq!(st.phase, Phase::Unknown);
    assert_eq!(st.before_unknown, Some(Phase::Firing));
    // back, still holding: firing again with no notice
    let n = run(&mut st, &mut at, &[holds()], &t);
    assert!(n.is_empty(), "{n:?}");
    assert_eq!(st.phase, Phase::Firing);
}

#[test]
fn time_spent_unreadable_does_not_count_toward_for() {
    let (mut st, mut at) = (AlertState::new(0), 0);
    run(&mut st, &mut at, &[holds(), holds()], &T);
    assert_eq!(st.progress, 60);
    // ten unreadable minutes: if they counted, the next holding reading
    // would fire
    run(&mut st, &mut at, &vec![missing(); 10], &T);
    let n = run(&mut st, &mut at, &[holds()], &T);
    assert!(n.is_empty(), "{n:?}");
    assert_eq!(st.phase, Phase::Pending);
    assert_eq!(st.progress, 60, "the gap added nothing");
    let n = run(&mut st, &mut at, &[holds(), holds(), holds(), holds()], &T);
    assert_eq!(n, [NoticeKind::Firing]);
}

#[test]
fn a_missing_value_is_noticed_once_after_the_wait_and_its_return_too() {
    let (mut st, mut at) = (AlertState::new(0), 0);
    let n = run(&mut st, &mut at, &vec![missing(); 10], &T);
    assert!(n.is_empty(), "before the wait: {n:?}");
    let n = run(&mut st, &mut at, &vec![missing(); 30], &T);
    assert_eq!(n, [NoticeKind::Unreadable], "once, not repeated");
    let n = run(&mut st, &mut at, &[clear()], &T);
    assert_eq!(n, [NoticeKind::Readable]);
}

#[test]
fn a_source_that_is_down_leaves_the_notice_to_the_source() {
    let (mut st, mut at) = (AlertState::new(0), 0);
    let n = run(&mut st, &mut at, &vec![down(); 40], &T);
    assert!(n.is_empty(), "{n:?}");
    assert_eq!(st.phase, Phase::Unknown);
}

#[test]
fn firing_repeats_at_the_interval_and_escalates_once() {
    let t = Timing { for_secs: 0, ..T };
    let (mut st, mut at) = (AlertState::new(0), 0);
    let hourly = |_: Severity| Some(3600);
    let mut notices = Vec::new();
    for i in 0..121 {
        let e = match i {
            90.. => Eval::Holds {
                value: "1".into(),
                threshold: "x".into(),
                severity: Some(Severity::Critical),
            },
            _ => holds(),
        };
        notices.extend(step(&mut st, &e, at, &t, hourly).notices);
        at += 60;
    }
    assert_eq!(
        notices,
        [
            NoticeKind::Firing,
            NoticeKind::Repeat,
            NoticeKind::Escalated,
        ],
        "firing at 0, repeat at 60 min, escalation at 90 min, the next repeat not due before 150"
    );
}

#[test]
fn a_rule_without_repeat_is_sent_once() {
    let t = Timing { for_secs: 0, ..T };
    let (mut st, mut at) = (AlertState::new(0), 0);
    let mut n = Vec::new();
    for _ in 0..3000 {
        n.extend(step(&mut st, &holds(), at, &t, |_| None).notices);
        at += 60;
    }
    assert_eq!(n, [NoticeKind::Firing]);
}
