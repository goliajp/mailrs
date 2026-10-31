//! The mail's two bodies. The HTML uses tables and inline styles only:
//! mail clients ignore style sheets and most of modern CSS.

use super::{Lang, Level, Reader, Shown};

struct Tone {
    bar: &'static str,
    badge_fg: &'static str,
    badge_bg: &'static str,
}

fn tone(level: Level) -> Tone {
    match level {
        Level::Act => Tone {
            bar: "#d92d20",
            badge_fg: "#b42318",
            badge_bg: "#fef3f2",
        },
        Level::Check => Tone {
            bar: "#f79009",
            badge_fg: "#b54708",
            badge_bg: "#fffaeb",
        },
        Level::Note => Tone {
            bar: "#2e90fa",
            badge_fg: "#175cd3",
            badge_bg: "#eff8ff",
        },
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
    what: &'static str,
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
            what: "这是什么情况",
            steps: "建议怎么做",
            when: "发生时间",
            colon: "：",
            footer: format!(
                "这封信发给本服务器设置的安全提醒收件人。同一件事对同一个账号或地址一小时内只提醒一次；重置码每签发一个就提醒一次。时间按 {clock} 显示。"
            ),
        },
        Lang::En => Words {
            kicker: "Security alert",
            what: "What this means",
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
    let font = "-apple-system,BlinkMacSystemFont,'Segoe UI','PingFang SC','Hiragino Sans','Microsoft YaHei',Roboto,Helvetica,Arial,sans-serif";
    let mut cards = String::new();
    for s in shown {
        cards.push_str(&card(s, host, &w, reader.lang));
    }
    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="light"><title>{title}</title></head>
<body style="margin:0;padding:0;background:#f2f4f7;font-family:{font};color:#101828;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#f2f4f7;">
<tr><td align="center" style="padding:28px 12px;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="max-width:600px;">
<tr><td style="padding:0 4px 16px 4px;">
<div style="font-size:13px;color:#667085;letter-spacing:0.02em;">{kicker} · {host}</div>
<div style="font-size:22px;line-height:30px;font-weight:700;color:#101828;margin-top:4px;">{summary}</div>
</td></tr>
{cards}<tr><td style="padding:8px 4px 0 4px;font-size:12px;line-height:18px;color:#98a2b3;">{footer}</td></tr>
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
    let t = tone(c.level);
    let mut rows = format!(
        r#"<tr><td style="padding:6px 0;font-size:13px;color:#667085;width:96px;vertical-align:top;">{}</td><td style="padding:6px 0;font-size:14px;color:#101828;vertical-align:top;">{}</td></tr>"#,
        esc(w.when),
        esc(&s.when)
    );
    for (k, v) in &s.facts {
        rows.push_str(&format!(
            r#"<tr><td style="padding:6px 0;font-size:13px;color:#667085;width:96px;vertical-align:top;border-top:1px solid #f2f4f7;">{}</td><td style="padding:6px 0;font-size:14px;color:#101828;vertical-align:top;border-top:1px solid #f2f4f7;overflow-wrap:break-word;">{}</td></tr>"#,
            esc(k),
            esc(v)
        ));
    }
    let steps: String = c
        .steps
        .iter()
        .map(|st| format!(r#"<li style="margin:0 0 6px 0;">{}</li>"#, esc(st)))
        .collect();
    let button = match &c.link {
        Some((path, label)) => format!(
            r#"<tr><td style="padding:4px 24px 22px 24px;"><a href="https://{host}{path}" style="display:inline-block;background:#101828;color:#ffffff;text-decoration:none;font-size:14px;font-weight:600;padding:10px 18px;border-radius:8px;">{label} →</a></td></tr>"#,
            host = esc(host),
            path = esc(path),
            label = esc(label),
        ),
        None => String::new(),
    };
    format!(
        r#"<tr><td style="padding:0 0 16px 0;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#ffffff;border-radius:12px;border:1px solid #eaecf0;border-left:4px solid {bar};">
<tr><td style="padding:20px 24px 4px 24px;">
<span style="display:inline-block;font-size:12px;font-weight:600;color:{badge_fg};background:{badge_bg};border-radius:999px;padding:3px 10px;">{level}</span>
<div style="font-size:18px;line-height:26px;font-weight:700;color:#101828;margin-top:10px;">{title}</div>
</td></tr>
<tr><td style="padding:8px 24px 0 24px;">
<div style="font-size:12px;font-weight:600;color:#667085;text-transform:uppercase;letter-spacing:0.04em;">{what}</div>
<div style="font-size:14px;line-height:22px;color:#344054;margin-top:4px;">{meaning}</div>
</td></tr>
<tr><td style="padding:14px 24px 0 24px;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#f9fafb;border-radius:8px;padding:6px 14px;">{rows}</table>
</td></tr>
<tr><td style="padding:16px 24px 8px 24px;">
<div style="font-size:12px;font-weight:600;color:#667085;text-transform:uppercase;letter-spacing:0.04em;">{steps_h}</div>
<ol style="margin:6px 0 0 0;padding-left:20px;font-size:14px;line-height:22px;color:#344054;">{steps}</ol>
</td></tr>
{button}</table>
</td></tr>
"#,
        bar = t.bar,
        badge_fg = t.badge_fg,
        badge_bg = t.badge_bg,
        level = esc(c.level.label(lang)),
        title = esc(&c.title),
        what = esc(w.what),
        meaning = esc(&c.meaning),
        steps_h = esc(w.steps),
    )
}
