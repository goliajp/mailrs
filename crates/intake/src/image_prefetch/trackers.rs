//! Which images in a received message only tell the sender it was read.
//!
//! The rules are data, in `trackers.json`, shared with the web client
//! (which keeps a byte-identical copy and hides the same images). An image
//! is a tracker when its host is a tracking service, its path is a known
//! open-tracking path, or it is drawn too small or hidden to be seen.

use std::sync::LazyLock;

struct Rules {
    hosts: Vec<String>,
    paths: Vec<String>,
}

static RULES: LazyLock<Rules> = LazyLock::new(|| {
    let v: serde_json::Value =
        serde_json::from_str(include_str!("trackers.json")).expect("trackers.json is valid");
    let list = |key: &str| -> Vec<String> {
        v[key]
            .as_array()
            .expect("trackers.json lists it")
            .iter()
            .filter_map(|s| s.as_str().map(str::to_string))
            .collect()
    };
    Rules {
        hosts: list("hosts"),
        paths: list("paths"),
    }
});

/// Whether the image `src` in the lowercased `<img …>` tag is a tracker.
pub fn is_tracking_image(src: &str, tag_lower: &str) -> bool {
    tracking_url(src) || invisible(tag_lower)
}

fn tracking_url(src: &str) -> bool {
    let rest = src.split_once("://").map_or(src, |(_, r)| r);
    let (host, path) = rest.split_once('/').map_or((rest, ""), |(h, p)| (h, p));
    let host = host
        .rsplit_once('@')
        .map_or(host, |(_, h)| h)
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let path =
        format!("/{}", path.split(['?', '#']).next().unwrap_or_default()).to_ascii_lowercase();
    RULES
        .hosts
        .iter()
        .any(|h| host == *h || host.ends_with(&format!(".{h}")))
        || RULES.paths.iter().any(|p| path.contains(p.as_str()))
}

/// Drawn at a pixel or less on each side it states, with at least one
/// side stated, or hidden outright.
fn invisible(tag: &str) -> bool {
    if tag.contains("display:none")
        || tag.contains("display: none")
        || tag.contains("visibility:hidden")
        || tag.contains("visibility: hidden")
    {
        return true;
    }
    let sides: Vec<Option<u32>> = ["width", "height"]
        .iter()
        .map(|side| attr_px(tag, side).or_else(|| style_px(tag, side)))
        .collect();
    sides.iter().any(Option::is_some) && sides.iter().flatten().all(|px| *px <= 1)
}

/// `width="1"`, `width=1`, `width='1px'`.
fn attr_px(tag: &str, name: &str) -> Option<u32> {
    let mut from = 0;
    while let Some(at) = tag[from..].find(name) {
        let start = from + at;
        from = start + name.len();
        let before_ok = tag[..start].ends_with(|c: char| c.is_whitespace());
        let after = tag[from..].trim_start();
        if before_ok && let Some(value) = after.strip_prefix('=') {
            return leading_px(value.trim_start().trim_start_matches(['"', '\'']));
        }
    }
    None
}

/// `style="…width:1px…"`.
fn style_px(tag: &str, name: &str) -> Option<u32> {
    let style_at = tag.find("style=")?;
    let style = &tag[style_at..];
    let at = style.find(&format!("{name}:"))?;
    let before = style[..at].chars().last();
    if before.is_some_and(|c| c == '-' || c.is_ascii_alphanumeric()) {
        return None;
    }
    leading_px(style[at + name.len() + 1..].trim_start())
}

fn leading_px(s: &str) -> Option<u32> {
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    let n: u32 = digits.parse().ok()?;
    let unit = &s[digits.len()..];
    match unit.starts_with('%') || unit.starts_with("em") || unit.starts_with("rem") {
        true => None,
        false => Some(n),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(src: &str, tag: &str) -> bool {
        is_tracking_image(src, &tag.to_ascii_lowercase())
    }

    #[test]
    fn tracking_services_and_paths_are_trackers() {
        assert!(check(
            "https://mailtrack.io/trace/mail/abc.png",
            "<img src=x>"
        ));
        assert!(check("https://t.yesware.com/t/abc/o.gif", "<img src=x>"));
        assert!(check(
            "https://mandrillapp.com/track/open.php?u=1",
            "<img src=x>"
        ));
        assert!(check(
            "https://u1.ct.sendgrid.net/wf/open?upn=x",
            "<img src=x>"
        ));
        assert!(!check("https://yesware.com/logo.png", "<img src=x>"));
        assert!(!check("https://cdn.shop.test/hero.jpg", "<img src=x>"));
    }

    #[test]
    fn invisible_images_are_trackers() {
        let src = "https://cdn.shop.test/p.gif";
        assert!(check(src, r#"<img src=x width="1" height="1">"#));
        assert!(check(src, "<img src=x width=0 height=0>"));
        assert!(check(src, r#"<img src=x height='1px'>"#));
        assert!(check(src, r#"<img src=x style="width:1px;height:1px">"#));
        assert!(check(src, r#"<img src=x style="display: none">"#));
        assert!(!check(src, r#"<img src=x width="600" height="1">"#));
        assert!(!check(src, r#"<img src=x width="100%">"#));
        assert!(!check(src, r#"<img src=x style="max-width:1px">"#));
        assert!(!check(src, "<img src=x>"));
    }

    /// The web client hides the same images; its copy must not drift.
    #[test]
    fn the_web_copy_of_the_rules_is_the_same_file() {
        let web = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../web/src/lib/trackers.json");
        let web = std::fs::read_to_string(&web).expect("web/src/lib/trackers.json");
        assert_eq!(web, include_str!("trackers.json"));
    }
}
