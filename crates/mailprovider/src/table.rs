//! The providers worth knowing by name.
//!
//! Short on purpose. A table that tries to hold every mail host in the
//! world is stale the week after it ships and hides the fact that
//! autodiscovery is the general answer; this holds the ones a person is
//! likely to add, where getting it wrong costs a support conversation.
//!
//! Hosts and ports are the providers' own published settings. What is
//! not published anywhere convenient — that Gmail's All Mail duplicates
//! the mailbox, that a QQ login password is refused with a message that
//! does not say why — is the part that earns the table its place.

use std::borrow::Cow;

use crate::{AuthKind, Endpoint, Preset, Protocol, SecretHelp, Tls};

/// Every preset, for a set-up screen to list and for tests to sweep.
pub static ALL: &[Preset] = &[GMAIL, OUTLOOK, YAHOO_JP, QQ, NETEASE_163, ICLOUD, FASTMAIL];

/// The preset for an address, if there is one.
pub fn preset_for(address: &str) -> Option<&'static Preset> {
    let (local, domain) = address.rsplit_once('@')?;
    if local.is_empty() {
        return None;
    }
    preset_for_domain(domain)
}

/// The preset for a domain, if there is one.
pub fn preset_for_domain(domain: &str) -> Option<&'static Preset> {
    let d = domain.trim().trim_matches('.').to_ascii_lowercase();
    if d.is_empty() || d.contains(' ') {
        return None;
    }
    ALL.iter().find(|p| p.domains.contains(&d.as_str()))
}

const fn ep(protocol: Protocol, host: &'static str, port: u16, tls: Tls) -> Endpoint {
    Endpoint {
        protocol,
        host: Cow::Borrowed(host),
        port,
        tls,
    }
}

const GMAIL: Preset = Preset {
    id: "gmail",
    label: "Gmail",
    domains: &["gmail.com", "googlemail.com"],
    imap: ep(Protocol::Imap, "imap.gmail.com", 993, Tls::Implicit),
    smtp: ep(Protocol::Smtp, "smtp.gmail.com", 587, Tls::StartTls),
    auth: AuthKind::OAuth2,
    secret_help: None,
    // Every message appears here as well as in its own folder, so
    // syncing it downloads the mailbox a second time. `[Gmail]/Bin`
    // and Spam are excluded for the same reason a person would: they
    // are Gmail's own views, not folders they filed anything into.
    skip_folders: &[
        "[Gmail]/All Mail",
        "[Gmail]/Bin",
        "[Gmail]/Trash",
        "[Gmail]/Spam",
    ],
};

const OUTLOOK: Preset = Preset {
    id: "outlook",
    label: "Outlook / Hotmail",
    domains: &[
        "outlook.com",
        "outlook.jp",
        "hotmail.com",
        "hotmail.co.jp",
        "live.com",
        "live.jp",
        "msn.com",
    ],
    imap: ep(Protocol::Imap, "outlook.office365.com", 993, Tls::Implicit),
    smtp: ep(Protocol::Smtp, "smtp-mail.outlook.com", 587, Tls::StartTls),
    auth: AuthKind::OAuth2,
    secret_help: None,
    skip_folders: &[],
};

const YAHOO_JP: Preset = Preset {
    id: "yahoo-jp",
    label: "Yahoo! JAPAN メール",
    domains: &["yahoo.co.jp", "ybb.ne.jp"],
    imap: ep(Protocol::Imap, "imap.mail.yahoo.co.jp", 993, Tls::Implicit),
    smtp: ep(Protocol::Smtp, "smtp.mail.yahoo.co.jp", 465, Tls::Implicit),
    auth: AuthKind::AppPassword,
    secret_help: Some(SecretHelp {
        what: "IMAP/SMTP 用のパスワード（Yahoo! JAPAN ID とは別）",
        url: "https://mail.yahoo.co.jp/config/mailprotect",
    }),
    skip_folders: &[],
};

const QQ: Preset = Preset {
    id: "qq",
    label: "QQ 邮箱",
    domains: &["qq.com", "vip.qq.com", "foxmail.com"],
    imap: ep(Protocol::Imap, "imap.qq.com", 993, Tls::Implicit),
    smtp: ep(Protocol::Smtp, "smtp.qq.com", 465, Tls::Implicit),
    auth: AuthKind::AppPassword,
    secret_help: Some(SecretHelp {
        what: "授权码（不是登录密码）",
        url: "https://service.mail.qq.com/detail/0/75",
    }),
    skip_folders: &[],
};

const NETEASE_163: Preset = Preset {
    id: "netease",
    label: "网易邮箱 (163 / 126)",
    domains: &["163.com", "126.com", "yeah.net"],
    imap: ep(Protocol::Imap, "imap.163.com", 993, Tls::Implicit),
    smtp: ep(Protocol::Smtp, "smtp.163.com", 465, Tls::Implicit),
    auth: AuthKind::AppPassword,
    secret_help: Some(SecretHelp {
        what: "客户端授权密码（不是登录密码）",
        url: "https://help.mail.163.com/faqDetail.do?code=d7a5dc8471cd0c0e8b4b8f4f8e49998b374173cfe9171305fa1ce630d7f67ac2a5feb28b66796d3b",
    }),
    skip_folders: &[],
};

const ICLOUD: Preset = Preset {
    id: "icloud",
    label: "iCloud Mail",
    domains: &["icloud.com", "me.com", "mac.com"],
    imap: ep(Protocol::Imap, "imap.mail.me.com", 993, Tls::Implicit),
    smtp: ep(Protocol::Smtp, "smtp.mail.me.com", 587, Tls::StartTls),
    auth: AuthKind::AppPassword,
    secret_help: Some(SecretHelp {
        what: "App-Specific Password",
        url: "https://account.apple.com/account/manage",
    }),
    skip_folders: &[],
};

const FASTMAIL: Preset = Preset {
    id: "fastmail",
    label: "Fastmail",
    domains: &["fastmail.com", "fastmail.fm"],
    imap: ep(Protocol::Imap, "imap.fastmail.com", 993, Tls::Implicit),
    smtp: ep(Protocol::Smtp, "smtp.fastmail.com", 465, Tls::Implicit),
    auth: AuthKind::AppPassword,
    secret_help: Some(SecretHelp {
        what: "App Password",
        url: "https://app.fastmail.com/settings/security/apppasswords",
    }),
    skip_folders: &[],
};
