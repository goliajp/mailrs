//! The mail's two bodies. The HTML uses tables and inline styles only:
//! mail clients ignore style sheets and most of modern CSS.

use super::{Lang, Level, Reader, Shown};

// The web app's palette (the gds light theme), so the mail looks like
// the product it comes from.
const BG: &str = "#fafafa";
const SURFACE: &str = "#ffffff";
const BORDER: &str = "#e4e4e7";
const FG: &str = "#09090b";
const FG_SECONDARY: &str = "#3f3f46";
const FG_MUTED: &str = "#71717a";
const ACCENT: &str = "#3b7ddd";
const FONT: &str = "Inter,'Noto Sans SC','PingFang SC','Hiragino Sans','Microsoft YaHei',system-ui,-apple-system,'Segoe UI',Roboto,Helvetica,Arial,sans-serif";

/// The level badge's text and background: the theme's danger, warning
/// and info colours on a light tint of each.
fn badge(level: Level) -> (&'static str, &'static str) {
    match level {
        Level::Act => ("#c92a2a", "#fdecec"),
        Level::Check => ("#b35c00", "#fdf1e3"),
        Level::Note => ("#2b6bc5", "#ebf2fc"),
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn count(shown: &[Shown], level: Level) -> usize {
    shown.iter().filter(|s| s.copy.level == level).count()
}

/// The first line a reader sees: what is asked of them.
fn summary(shown: &[Shown], lang: Lang) -> String {
    let (act, check) = (count(shown, Level::Act), count(shown, Level::Check));
    match lang {
        Lang::Zh if act > 0 => format!("有 {act} 件事需要你处理"),
        Lang::Zh if check > 0 => format!("有 {check} 件事请留意"),
        Lang::Zh => "以下是通知，不需要处理".into(),
        Lang::En if act == 1 => "One thing needs your action".into(),
        Lang::En if act > 1 => format!("{act} things need your action"),
        Lang::En if check > 0 => "Worth a look".into(),
        Lang::En => "For your information; nothing needs doing".into(),
    }
}

struct Words {
    kicker: &'static str,
    steps: &'static str,
    when: &'static str,
    colon: &'static str,
    footer: String,
}

fn words(reader: &Reader) -> Words {
    let clock = &reader.clock.name;
    match reader.lang {
        Lang::Zh => Words {
            kicker: "安全提醒",
            steps: "建议怎么做",
            when: "发生时间",
            colon: "：",
            footer: format!(
                "这封信发给本服务器设置的安全提醒收件人。同一件事对同一个账号或地址一小时内只提醒一次；重置码每签发一个就提醒一次。时间按 {clock} 显示。"
            ),
        },
        Lang::En => Words {
            kicker: "Security alert",
            steps: "What to do",
            when: "When",
            colon: ": ",
            footer: format!(
                "Sent to this server's security alert recipients. The same alert about one account or address is sent at most once an hour; every reset code issued is reported. Times are in {clock}."
            ),
        },
    }
}

pub fn text(shown: &[Shown], host: &str, reader: &Reader) -> String {
    let w = words(reader);
    let mut out = format!("{} · {host}\n{}\n", w.kicker, summary(shown, reader.lang));
    for (i, s) in shown.iter().enumerate() {
        let c = &s.copy;
        out.push_str(&format!(
            "\n{}. [{}] {}\n\n{}\n\n",
            i + 1,
            c.level.label(reader.lang),
            c.title,
            c.meaning
        ));
        out.push_str(&format!("  {}{}{}\n", w.when, w.colon, s.when));
        for (k, v) in &s.facts {
            out.push_str(&format!("  {k}{}{v}\n", w.colon));
        }
        out.push_str(&format!("\n{}{}\n", w.steps, w.colon));
        for step in &c.steps {
            out.push_str(&format!("  - {step}\n"));
        }
        if let Some((path, label)) = &c.link {
            out.push_str(&format!("\n{label}{}https://{host}{path}\n", w.colon));
        }
    }
    out.push_str(&format!("\n--\n{}\n", w.footer));
    out
}

pub fn html(shown: &[Shown], host: &str, reader: &Reader) -> String {
    let w = words(reader);
    let mut cards = String::new();
    for s in shown {
        cards.push_str(&card(s, host, &w, reader.lang));
    }
    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="light"><title>{title}</title></head>
<body style="margin:0;padding:0;background:{BG};font-family:{FONT};color:{FG};">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:{BG};">
<tr><td align="center" style="padding:32px 12px;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="max-width:560px;">
<tr><td style="padding:0 2px 16px 2px;">
<div style="font-size:13px;line-height:18px;color:{FG_MUTED};">{kicker} · {host}</div>
<div style="font-size:20px;line-height:28px;font-weight:600;color:{FG};margin-top:4px;">{summary}</div>
</td></tr>
{cards}<tr><td style="padding:8px 2px 0 2px;font-size:12px;line-height:18px;color:{FG_MUTED};">{footer}</td></tr>
</table>
</td></tr>
</table>
</body></html>
"#,
        title = esc(&summary(shown, reader.lang)),
        kicker = esc(w.kicker),
        host = esc(host),
        summary = esc(&summary(shown, reader.lang)),
        footer = esc(&w.footer),
    )
}

fn card(s: &Shown, host: &str, w: &Words, lang: Lang) -> String {
    let c = &s.copy;
    let (badge_fg, badge_bg) = badge(c.level);
    let row = |k: &str, v: &str, first: bool| {
        let line = match first {
            true => "",
            false => "border-top:1px solid #f4f4f5;",
        };
        format!(
            r#"<tr><td style="padding:8px 12px 8px 0;font-size:13px;line-height:20px;color:{FG_MUTED};width:96px;vertical-align:top;{line}">{}</td><td style="padding:8px 0;font-size:14px;line-height:20px;color:{FG};vertical-align:top;overflow-wrap:break-word;{line}">{}</td></tr>"#,
            esc(k),
            esc(v)
        )
    };
    let mut rows = row(w.when, &s.when, true);
    for (k, v) in &s.facts {
        rows.push_str(&row(k, v, false));
    }
    let steps: String = c
        .steps
        .iter()
        .map(|st| format!(r#"<li style="margin:0 0 4px 0;">{}</li>"#, esc(st)))
        .collect();
    let button = match &c.link {
        Some((path, label)) => format!(
            r#"<tr><td style="padding:8px 24px 24px 24px;"><a href="https://{host}{path}" style="display:inline-block;background:{ACCENT};color:#ffffff;text-decoration:none;font-size:14px;line-height:20px;font-weight:500;padding:8px 16px;border-radius:6px;">{label}</a></td></tr>"#,
            host = esc(host),
            path = esc(path),
            label = esc(label),
        ),
        None => String::new(),
    };
    format!(
        r#"<tr><td style="padding:0 0 16px 0;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:{SURFACE};border:1px solid {BORDER};border-radius:12px;">
<tr><td style="padding:20px 24px 0 24px;">
<span style="display:inline-block;font-size:12px;line-height:18px;font-weight:500;color:{badge_fg};background:{badge_bg};border-radius:9999px;padding:1px 8px;">{level}</span>
<div style="font-size:16px;line-height:24px;font-weight:600;color:{FG};margin-top:8px;">{title}</div>
<div style="font-size:14px;line-height:22px;color:{FG_SECONDARY};margin-top:4px;">{meaning}</div>
</td></tr>
<tr><td style="padding:12px 24px 0 24px;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="border-top:1px solid {BORDER};border-bottom:1px solid {BORDER};">{rows}</table>
</td></tr>
<tr><td style="padding:16px 24px 8px 24px;">
<div style="font-size:13px;line-height:20px;font-weight:500;color:{FG};">{steps_h}</div>
<ol style="margin:4px 0 0 0;padding-left:20px;font-size:14px;line-height:22px;color:{FG_SECONDARY};">{steps}</ol>
</td></tr>
{button}</table>
</td></tr>
"#,
        level = esc(c.level.label(lang)),
        title = esc(&c.title),
        meaning = esc(&c.meaning),
        steps_h = esc(w.steps),
    )
}
