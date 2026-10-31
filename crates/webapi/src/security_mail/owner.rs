//! What an account's owner is told about their own account: a new device
//! signed in, a credential changed, mail now goes elsewhere. Same layout as
//! the operators' alerts, written for someone who did not design the server.

use super::copy::Copy;
use super::layout::{self, Audience};
use super::{Lang, Level, Mail, Reader, Shown, when};

/// Something that happened to the reader's account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// Signed in from a device not seen on this account before.
    NewSignIn,
    PasswordChanged,
    TwoStepOn,
    TwoStepOff,
    /// The recovery address is now this (empty: removed).
    RecoveryChanged(String),
    /// Mail is now forwarded to this address.
    ForwardingOn(String),
    /// A key for programs was made, with this name.
    AgentKeyMade(String),
    /// An app password for a mail app was made, with this name.
    AppPasswordMade(String),
    /// A session failed its password confirmations and was ended.
    SessionEnded,
}

/// Where and how it happened, when known.
#[derive(Debug, Clone, Default)]
pub struct Origin {
    pub ip: String,
    pub user_agent: String,
}

fn copy(n: &Notice, lang: Lang) -> Copy {
    let zh = lang == Lang::Zh;
    let not_you = match zh {
        true => "如果不是你本人：立刻修改密码，并在「设置 → 安全」里登出其他会话".to_string(),
        false => "If this was not you: change your password now, and sign out the other sessions in Settings → Security".to_string(),
    };
    let you = match zh {
        true => "如果是你本人，不用做什么".to_string(),
        false => "If this was you, there is nothing to do".to_string(),
    };
    let (level, title, meaning) = match (n, zh) {
        (Notice::NewSignIn, true) => (Level::Check, "新设备登录了你的账号".to_string(), "你的账号刚刚在一台之前没有用过的设备或浏览器上登录。".to_string()),
        (Notice::NewSignIn, false) => (Level::Check, "A new device signed in to your account".to_string(), "Your account was just signed in to from a device or browser it has not used before.".to_string()),
        (Notice::PasswordChanged, true) => (Level::Check, "你的密码已修改".to_string(), "你的账号密码刚刚被修改，其他会话已经登出。".to_string()),
        (Notice::PasswordChanged, false) => (Level::Check, "Your password was changed".to_string(), "Your account's password was just changed, and its other sessions were signed out.".to_string()),
        (Notice::TwoStepOn, true) => (Level::Note, "两步验证已开启".to_string(), "登录你的账号现在除了密码还要验证码。".to_string()),
        (Notice::TwoStepOn, false) => (Level::Note, "Two-step verification is on".to_string(), "Signing in to your account now takes a code as well as the password.".to_string()),
        (Notice::TwoStepOff, true) => (Level::Act, "两步验证已关闭".to_string(), "登录你的账号现在只要密码。".to_string()),
        (Notice::TwoStepOff, false) => (Level::Act, "Two-step verification was turned off".to_string(), "Signing in to your account now takes only the password.".to_string()),
        (Notice::RecoveryChanged(to), true) => (Level::Check, "恢复邮箱已修改".to_string(), match to.is_empty() { true => "你的账号已经没有恢复邮箱。".to_string(), false => format!("你的账号的恢复邮箱现在是 {to}。") }),
        (Notice::RecoveryChanged(to), false) => (Level::Check, "Your recovery address was changed".to_string(), match to.is_empty() { true => "Your account no longer has a recovery address.".to_string(), false => format!("Your account's recovery address is now {to}.") }),
        (Notice::ForwardingOn(to), true) => (Level::Act, "你的邮件开始转发".to_string(), format!("寄给你的邮件现在会转发到 {to}。")),
        (Notice::ForwardingOn(to), false) => (Level::Act, "Your mail is now forwarded".to_string(), format!("Mail sent to you is now forwarded to {to}.")),
        (Notice::AgentKeyMade(name), true) => (Level::Check, "新建了一把程序用的 key".to_string(), format!("你的账号新建了一把给程序用的 key「{name}」，持有它的程序可以按它的权限读写你的邮件。")),
        (Notice::AgentKeyMade(name), false) => (Level::Check, "A key for programs was made".to_string(), format!("A key for programs named \"{name}\" was made on your account; a program holding it can use your mail as far as its scopes allow.")),
        (Notice::AppPasswordMade(name), true) => (Level::Check, "新建了一个应用专用密码".to_string(), format!("你的账号新建了一个应用专用密码「{name}」，用它可以在邮件 App 里收发你的邮件。")),
        (Notice::AppPasswordMade(name), false) => (Level::Check, "An app password was made".to_string(), format!("An app password named \"{name}\" was made on your account; a mail app using it can read and send your mail.")),
        (Notice::SessionEnded, true) => (Level::Act, "一个会话因为确认密码失败被结束".to_string(), "你账号的一个已登录会话连续 5 次输错了确认密码，已经被结束。持有它的人不知道你的密码。".to_string()),
        (Notice::SessionEnded, false) => (Level::Act, "A session was ended after failed password confirmations".to_string(), "A signed-in session on your account entered the wrong password five times when asked to confirm it, and was ended: whoever held it did not know the password.".to_string()),
    };
    let steps = match n {
        Notice::SessionEnded => vec![not_you],
        _ => vec![you, not_you],
    };
    let label = match zh {
        true => "打开安全设置",
        false => "Open security settings",
    };
    Copy {
        level,
        title,
        meaning,
        steps,
        link: Some(("/settings?tab=security".to_string(), label)),
    }
}

fn facts(origin: &Origin, lang: Lang) -> Vec<(String, String)> {
    let (ip, device) = match lang {
        Lang::Zh => ("来源地址", "设备"),
        Lang::En => ("From", "Device"),
    };
    let mut out = Vec::new();
    if !origin.ip.is_empty() {
        out.push((ip.to_string(), origin.ip.clone()));
    }
    if !origin.user_agent.is_empty() {
        out.push((device.to_string(), origin.user_agent.clone()));
    }
    out
}

/// The mail telling an account's owner about `notice`.
pub fn owner_mail(notice: &Notice, origin: &Origin, host: &str, reader: &Reader, now: u64) -> Mail {
    let shown = [Shown {
        copy: copy(notice, reader.lang),
        when: when(now, reader, now),
        facts: facts(origin, reader.lang),
    }];
    let subject = match reader.lang {
        Lang::Zh => format!("[{host}] 安全提醒：{}", shown[0].copy.title),
        Lang::En => format!("[{host}] Security: {}", shown[0].copy.title),
    };
    Mail {
        subject,
        text: layout::text(&shown, host, reader, Audience::Owner),
        html: layout::html(&shown, host, reader, Audience::Owner),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reader(lang: Lang) -> Reader {
        Reader {
            lang,
            clock: super::super::parse_offset("+09:00").unwrap(),
        }
    }

    #[test]
    fn a_new_sign_in_says_where_from_and_what_to_do() {
        let origin = Origin {
            ip: "203.0.113.9".into(),
            user_agent: "Firefox".into(),
        };
        let mail = owner_mail(
            &Notice::NewSignIn,
            &origin,
            "mail.example",
            &reader(Lang::Zh),
            0,
        );
        assert!(
            mail.subject.contains("新设备登录了你的账号"),
            "{}",
            mail.subject
        );
        assert!(mail.text.contains("203.0.113.9"));
        assert!(mail.text.contains("登出其他会话"));
        assert!(mail.text.contains("账号本人"), "{}", mail.text);
        assert!(!mail.text.contains("安全提醒收件人"));
        let en = owner_mail(
            &Notice::ForwardingOn("x@y.example".into()),
            &Origin::default(),
            "mail.example",
            &reader(Lang::En),
            0,
        );
        assert!(en.text.contains("forwarded to x@y.example"));
        assert!(en.html.contains("/settings?tab=security"));
    }
}
