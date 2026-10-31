//! What each kind of alert says: a headline, how urgent it is, what it
//! means for someone who did not design the login guard, and what to do.

use mailrs_auth_guard::alerts::{Alert, Fact, Kind};

use super::Lang;

/// How urgent an alert is, in the order a reader should see them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Act,
    Check,
    Note,
}

impl Level {
    pub fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Level::Act, Lang::Zh) => "需要处理",
            (Level::Check, Lang::Zh) => "请留意",
            (Level::Note, Lang::Zh) => "知悉即可",
            (Level::Act, Lang::En) => "Action needed",
            (Level::Check, Lang::En) => "Worth a look",
            (Level::Note, Lang::En) => "For your information",
        }
    }
}

/// The words for one alert.
pub struct Copy {
    pub level: Level,
    pub title: String,
    pub meaning: String,
    pub steps: Vec<String>,
    /// A page of the admin console that shows more, and its button text.
    pub link: Option<(String, &'static str)>,
}

fn issuer(a: &Alert) -> String {
    a.facts
        .iter()
        .find_map(|f| match f {
            Fact::IssuedBy { actor } => Some(actor.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn sign_ins(lang: Lang, query: &str) -> Option<(String, &'static str)> {
    let label = match lang {
        Lang::Zh => "查看登录记录",
        Lang::En => "Open sign-in records",
    };
    Some((format!("/admin/sign-ins{query}"), label))
}

pub fn copy(a: &Alert, lang: Lang) -> Copy {
    let s = a.subject.as_str();
    let account = format!("?account={}", urlencode(s));
    match lang {
        Lang::Zh => zh(a, s, &account),
        Lang::En => en(a, s, &account),
    }
}

fn zh(a: &Alert, s: &str, account: &str) -> Copy {
    let v = |x: &[&str]| x.iter().map(|t| t.to_string()).collect();
    match a.kind {
        Kind::AccountLocked => Copy {
            level: Level::Check,
            title: format!("有人在猜 {s} 的密码"),
            meaning: "短时间内有好几个不同的地址用错误的密码登录这个账号。系统已经暂时只允许它从常用的地方登录，账号的主人在自己常用的设备上照常使用。".into(),
            steps: v(&[
                "如果账号的主人刚换了设备、或者忘了密码在反复试，可以不用管。",
                "如果不是，请让他换一个更长的密码，并打开两步验证。",
            ]),
            link: sign_ins(Lang::Zh, account),
        },
        Kind::SourceLocked => Copy {
            level: Level::Note,
            title: format!("地址 {s} 在试很多个账号，已被暂时拦下"),
            meaning: "同一个地址在短时间内对好几个账号登录失败，这通常是网上的自动扫描。锁定期间这个地址的所有登录都会被拒绝，其他人不受影响。".into(),
            steps: v(&[
                "一般不需要处理。",
                "如果这是你们公司的出口地址（很多人共用一个），说明有人在反复输错密码，可以在登录记录里看是哪些账号。",
            ]),
            link: sign_ins(Lang::Zh, &format!("?source={}", urlencode(s))),
        },
        Kind::GlobalLocked => Copy {
            level: Level::Act,
            title: "整台服务器登录失败太多，已暂停陌生地址登录".into(),
            meaning: "所有账号加起来的登录失败超过了上限，可能有人在大规模猜密码。在失败数降下来之前，没有在某个账号上成功登录过的地址都会被拒绝；已经在用的设备照常登录。".into(),
            steps: v(&[
                "看一下登录记录，确认是不是集中的攻击、针对的是哪些账号。",
                "如果有人反映登录不了，请他用平时用的设备登录，或者过一会儿再试。",
            ]),
            link: sign_ins(Lang::Zh, ""),
        },
        Kind::ManyNetworks => Copy {
            level: Level::Act,
            title: format!("{s} 几分钟内从好几个网络都登录成功了"),
            meaning: "这个账号在很短的时间里从几个不同的网络用正确的密码登录。如果不是本人在换网络（比如手机和电脑同时在用），密码可能已经泄露了。".into(),
            steps: v(&[
                "直接问账号的主人，这些登录是不是他自己。",
                "如果不是，在管理后台对这个账号做「账号被盗」重置：换密码、结束所有登录、吊销它的 agent key。",
            ]),
            link: sign_ins(Lang::Zh, account),
        },
        Kind::SessionEndedAfterFailedConfirmations => Copy {
            level: Level::Check,
            title: format!("{s} 的一个登录被强制结束了"),
            meaning: "有人用着这个账号已经登录的页面，在做敏感操作前需要再输一次密码，但连续输错了。系统已经结束了这次登录。用着它的人可能不是账号的主人。".into(),
            steps: v(&[
                "问账号的主人是不是自己输错了。",
                "如果不是，对这个账号做「账号被盗」重置。",
            ]),
            link: sign_ins(Lang::Zh, account),
        },
        Kind::ResetCodeIssued => Copy {
            level: Level::Note,
            title: format!("{} 给 {s} 签发了一个重置码", issuer(a)),
            meaning: "拿到重置码的人可以给这个账号设一个新密码。每签发一个都会发这封通知，方便你确认是不是你们安排的。".into(),
            steps: v(&[
                "如果是你们安排的，不需要处理。",
                "如果不是，马上联系签发人确认；确认不了，就对签发人的账号做「账号被盗」重置。",
            ]),
            link: None,
        },
        Kind::OutboundPaused => Copy {
            level: Level::Act,
            title: format!("{s} 的外发邮件已暂停"),
            meaning: "这个账号一小时内发出的邮件或者退信超过了上限，系统已经暂停它往外发信。之后它发的邮件会先扣下，不会发出去。常见原因是账号被盗用来发垃圾邮件，或者批量发信的设置有误。暂停期间每天提醒一次。".into(),
            steps: v(&[
                "先确认这些邮件是不是账号的主人自己发的。",
                "确认没问题后，在「管理 → 队列 → 已暂停的发件人」里恢复，可以选择把扣下的邮件继续发出。",
                "如果是被盗用，先对这个账号做「账号被盗」重置，再恢复并选择丢弃扣下的邮件。",
            ]),
            link: Some(("/admin/queues".into(), "打开已暂停的发件人")),
        },
        Kind::BackupFailed => Copy {
            level: Level::Act,
            title: format!("{s} 的备份没有成功"),
            meaning: "每天的自动备份失败了。修好之前，新收到的邮件和改过的设置都没有备份。".into(),
            steps: v(&[
                "在服务器上看备份容器的日志：docker logs mailrs-backup",
                "修好以后，等下一次自动备份成功，会不再提醒。",
            ]),
            link: None,
        },
        Kind::ComplaintLimit => Copy {
            level: Level::Check,
            title: format!("{s} 今天转来的投诉超过了上限"),
            meaning: "收件人在他们的邮箱里把我们的信标记为垃圾邮件，邮件服务商会把这些投诉转给我们，我们会自动不再给投诉的人发信。今天这个服务商的投诉超过了每天处理的上限，后面的投诉只记录、不再自动处理。".into(),
            steps: v(&[
                "看一下最近谁在大量发信，是否有收件人不想收的群发。",
                "如果是正常的业务邮件，考虑在信里把退订做得更明显。",
            ]),
            link: None,
        },
    }
}

fn en(a: &Alert, s: &str, account: &str) -> Copy {
    let v = |x: &[&str]| x.iter().map(|t| t.to_string()).collect();
    match a.kind {
        Kind::AccountLocked => Copy {
            level: Level::Check,
            title: format!("Someone is guessing the password of {s}"),
            meaning: "Several different addresses failed to sign in to this account within a short time. It now accepts sign-ins only from places it has signed in from before, so its owner keeps working on their usual devices.".into(),
            steps: v(&[
                "If the owner has just changed devices or forgotten the password, nothing needs doing.",
                "Otherwise, ask them to choose a longer password and turn on two-step verification.",
            ]),
            link: sign_ins(Lang::En, account),
        },
        Kind::SourceLocked => Copy {
            level: Level::Note,
            title: format!("{s} tried many accounts and is blocked for now"),
            meaning: "One address failed to sign in to several accounts within a short time, which is usually an automated scan. Every sign-in from it is refused while the block lasts; nobody else is affected.".into(),
            steps: v(&[
                "Usually nothing needs doing.",
                "If it is your office's shared address, someone is mistyping a password; the sign-in records show which accounts.",
            ]),
            link: sign_ins(Lang::En, &format!("?source={}", urlencode(s))),
        },
        Kind::GlobalLocked => Copy {
            level: Level::Act,
            title: "Too many failed sign-ins server-wide; unfamiliar addresses are refused".into(),
            meaning: "Failed sign-ins across all accounts passed the limit, which may be a large guessing attack. Until the count falls, addresses that never signed in to the account they try are refused; devices already in use keep working.".into(),
            steps: v(&[
                "Check the sign-in records for an attack and the accounts it targets.",
                "If someone cannot sign in, ask them to use their usual device or try again later.",
            ]),
            link: sign_ins(Lang::En, ""),
        },
        Kind::ManyNetworks => Copy {
            level: Level::Act,
            title: format!("{s} signed in from several networks within minutes"),
            meaning: "This account signed in with the right password from several networks in a short time. Unless its owner is switching networks (a phone and a laptop at once), the password may have leaked.".into(),
            steps: v(&[
                "Ask the owner whether these sign-ins were theirs.",
                "If not, use the compromised-account reset in the admin console: new password, every sign-in ended, agent keys revoked.",
            ]),
            link: sign_ins(Lang::En, account),
        },
        Kind::SessionEndedAfterFailedConfirmations => Copy {
            level: Level::Check,
            title: format!("A sign-in of {s} was ended"),
            meaning: "Someone using this account's signed-in page had to enter the password again before a sensitive change, and got it wrong repeatedly. The sign-in was ended; whoever held it may not be the owner.".into(),
            steps: v(&[
                "Ask the owner whether it was them.",
                "If not, use the compromised-account reset on this account.",
            ]),
            link: sign_ins(Lang::En, account),
        },
        Kind::ResetCodeIssued => Copy {
            level: Level::Note,
            title: format!("{} issued a reset code for {s}", issuer(a)),
            meaning: "Whoever holds a reset code can set a new password on this account. Every code issued is reported, so you can tell whether it was planned.".into(),
            steps: v(&[
                "If you arranged it, nothing needs doing.",
                "If not, contact the issuer at once; if that fails, use the compromised-account reset on the issuer's account.",
            ]),
            link: None,
        },
        Kind::OutboundPaused => Copy {
            level: Level::Act,
            title: format!("Outbound mail from {s} is paused"),
            meaning: "This account sent, or bounced, more mail in an hour than its limit, so its outbound mail is paused: what it sends now is held, not delivered. Usually the account was taken over to send spam, or a bulk mailing was misconfigured. You are reminded daily while it lasts.".into(),
            steps: v(&[
                "Check whether the owner sent this mail.",
                "If all is well, resume it under Admin → Queues → Paused senders, and choose to send the held mail.",
                "If the account was taken over, reset it as compromised first, then resume and discard the held mail.",
            ]),
            link: Some(("/admin/queues".into(), "Open paused senders")),
        },
        Kind::BackupFailed => Copy {
            level: Level::Act,
            title: format!("Backup of {s} failed"),
            meaning: "The daily backup did not succeed. Until it does, new mail and changed settings are not backed up.".into(),
            steps: v(&[
                "On the server, read the backup container's log: docker logs mailrs-backup",
                "Once fixed, the reminders stop with the next successful backup.",
            ]),
            link: None,
        },
        Kind::ComplaintLimit => Copy {
            level: Level::Check,
            title: format!("{s} sent more complaints today than are handled"),
            meaning: "When recipients mark our mail as spam, their provider forwards the complaint and we stop mailing them. This provider passed today's limit; further complaints are recorded but not acted on.".into(),
            steps: v(&[
                "Check who has been sending a lot, and whether anyone sent mail its recipients did not want.",
                "If it is ordinary business mail, make unsubscribing easier to find.",
            ]),
            link: None,
        },
    }
}

/// Percent-encode a query value.
fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
