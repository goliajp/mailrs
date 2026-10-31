use mailrs_auth_guard::alerts::{Alert, Fact, Kind};

use super::*;

const NOW: u64 = 1_793_427_570; // 2026-10-31 06:19:30 UTC

fn reader(lang: Lang) -> Reader {
    Reader {
        lang,
        clock: parse_offset("+09:00").unwrap(),
    }
}

fn reset_code() -> Alert {
    Alert {
        kind: Kind::ResetCodeIssued,
        subject: "qa@golia.jp".into(),
        at: NOW,
        detail: "issued by devops@golia.jp, valid until 1793513971 (unix seconds)".into(),
        facts: vec![
            Fact::IssuedBy {
                actor: "devops@golia.jp".into(),
            },
            Fact::ValidUntil { at: 1_793_513_971 },
        ],
    }
}

fn paused() -> Alert {
    Alert {
        kind: Kind::OutboundPaused,
        subject: "sales@golia.jp".into(),
        at: NOW - 600,
        detail: String::new(),
        facts: vec![Fact::SendLimit {
            count: 250,
            limit: 200,
        }],
    }
}

/// The reset code alert as it came to prod: "valid until 1793513971 (unix
/// seconds)". Now a date in the reader's zone and how far off.
#[test]
fn times_are_dates_in_the_readers_zone_not_unix_seconds() {
    let mail = render(&[reset_code()], "mail.golia.ai", &reader(Lang::Zh), NOW);
    assert_eq!(
        mail.subject,
        "[mail.golia.ai] 安全提醒：devops@golia.jp 给 qa@golia.jp 签发了一个重置码"
    );
    for body in [&mail.text, &mail.html] {
        assert!(!body.contains("1793513971"), "unix seconds in:\n{body}");
        assert!(
            body.contains("2026-11-01 15:19（UTC+9），约 24 小时后"),
            "{body}"
        );
        assert!(body.contains("2026-10-31 15:19（UTC+9），刚刚"), "{body}");
        assert!(body.contains("知悉即可"), "{body}");
    }
}

/// Several alerts: the subject counts what needs action, and what needs
/// action comes first.
#[test]
fn what_needs_action_comes_first() {
    let mail = render(
        &[reset_code(), paused()],
        "mail.golia.ai",
        &reader(Lang::Zh),
        NOW,
    );
    assert_eq!(
        mail.subject,
        "[mail.golia.ai] 安全提醒：2 件事，其中 1 件需要处理"
    );
    let (paused_at, code_at) = (
        mail.text.find("sales@golia.jp 的外发邮件已暂停").unwrap(),
        mail.text.find("签发了一个重置码").unwrap(),
    );
    assert!(paused_at < code_at, "{}", mail.text);
    assert!(mail.html.contains("有 1 件事需要你处理"));
    assert!(mail.text.contains("250 位收件人（上限 200）"));
    assert!(mail.text.contains("https://mail.golia.ai/admin/queues"));
}

#[test]
fn english_reads_too() {
    let mail = render(&[reset_code()], "mail.golia.ai", &reader(Lang::En), NOW);
    assert_eq!(
        mail.subject,
        "[mail.golia.ai] Security: devops@golia.jp issued a reset code for qa@golia.jp"
    );
    assert!(
        mail.text
            .contains("2026-11-01 15:19 (UTC+9), in about 24 hours")
    );
}

/// What a sender or an attacker chose (an address, a source) is text in
/// the HTML, never markup.
#[test]
fn values_are_escaped_in_the_html() {
    let mut a = paused();
    a.subject = "<script>x</script>@golia.jp".into();
    let mail = render(&[a], "mail.golia.ai", &reader(Lang::En), NOW);
    assert!(!mail.html.contains("<script>"));
    assert!(mail.html.contains("&lt;script&gt;"));
}

/// An alert queued before facts existed shows its English line.
#[test]
fn an_alert_without_facts_shows_its_line() {
    let mut a = reset_code();
    a.facts.clear();
    let mail = render(&[a], "mail.golia.ai", &reader(Lang::Zh), NOW);
    assert!(mail.text.contains("详情：issued by devops@golia.jp"));
}

#[test]
fn offsets_and_durations_read_as_people_say_them() {
    assert_eq!(parse_offset("+09:00").unwrap().name, "UTC+9");
    assert_eq!(
        parse_offset("-05:30").unwrap().offset_secs,
        -(5 * 3600 + 30 * 60)
    );
    assert!(parse_offset("JST").is_none());
    assert_eq!(relative(-3 * 86_400, Lang::Zh), "3 天前");
    assert_eq!(relative(90, Lang::En), "in 1 minute");
    assert_eq!(duration(30 * 60, Lang::Zh), "30 分钟");
}

/// Every kind has words in both languages and a level.
#[test]
fn every_kind_is_written() {
    for kind in [
        Kind::AccountLocked,
        Kind::SourceLocked,
        Kind::GlobalLocked,
        Kind::ManyNetworks,
        Kind::SessionEndedAfterFailedConfirmations,
        Kind::ResetCodeIssued,
        Kind::BackupFailed,
        Kind::OutboundPaused,
        Kind::ComplaintLimit,
    ] {
        for lang in [Lang::Zh, Lang::En] {
            let mut a = reset_code();
            a.kind = kind;
            let mail = render(&[a], "h", &reader(lang), NOW);
            assert!(
                !mail.subject.is_empty() && !mail.html.is_empty(),
                "{kind:?}"
            );
        }
    }
}
