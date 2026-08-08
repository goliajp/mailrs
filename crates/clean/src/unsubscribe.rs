//! `List-Unsubscribe` (RFC 2369) and one-click (RFC 8058).
//!
//! The neighbour of [`crate::detect_bulk_sender`], which answers whether
//! a message is bulk by noticing this header exists. This answers the
//! next question: where does unsubscribing actually go, and can it be
//! done without a browser.
//!
//! Written against 13,441 real headers rather than the RFC alone. What
//! the corpus says, and the RFC does not:
//!
//! - **221 have no angle brackets.** RFC 2369 requires them; Microsoft
//!   and Slack send a bare URL anyway. A parser that demands `<>` drops
//!   1.6% of the unsubscribe links that exist.
//! - **19 have a comma inside the URL.** Splitting the value on commas
//!   — the obvious reading of "comma-separated list" — cuts those in
//!   half. The separator only counts outside the brackets.
//! - **73 spell the one-click token differently**: `One-click`, and six
//!   that write the key as `List-Unsubscribe-Post` again. Matching the
//!   RFC's exact casing loses them.
//! - **Five are unexpanded merge tags** — `%%=concat(cloudpagesurl(…`,
//!   an ESP template that never ran. They are not URIs and must not be
//!   offered as one.

/// Where a message says unsubscribing goes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Unsubscribe {
    /// `http(s):` targets, in the order the header gave them.
    pub http: Vec<String>,
    /// `mailto:` targets, in the order the header gave them.
    pub mailto: Vec<String>,
    /// The sender accepts an RFC 8058 one-click POST to the first
    /// `http` target, so unsubscribing needs no browser and no
    /// confirmation page.
    pub one_click: bool,
}

impl Unsubscribe {
    /// The https URI a one-click POST may be sent to, if there is one.
    ///
    /// Only `https`. RFC 8058 §3 requires it, and 304 messages in the
    /// corpus offer plain `http` — posting the opaque subscriber token
    /// over that would hand it to anyone on the path, to save the
    /// reader one tap.
    pub fn one_click_url(&self) -> Option<&str> {
        if !self.one_click {
            return None;
        }
        self.http
            .iter()
            .find(|u| u.len() >= 8 && u[..8].eq_ignore_ascii_case("https://"))
            .map(String::as_str)
    }

    /// Nothing usable was found.
    pub fn is_empty(&self) -> bool {
        self.http.is_empty() && self.mailto.is_empty()
    }
}

/// Parse `List-Unsubscribe`, with `List-Unsubscribe-Post` when present.
///
/// Both values are the unfolded header content, without the field name.
/// Returns `None` when nothing in the header is a URI this can offer —
/// which is a different answer from "the header was absent", and the
/// caller wants both to look the same.
pub fn parse_unsubscribe(value: &str, post: Option<&str>) -> Option<Unsubscribe> {
    let mut found = Unsubscribe {
        one_click: post.is_some_and(is_one_click),
        ..Default::default()
    };
    for target in targets(value) {
        match scheme_of(target) {
            // A URI cannot contain a space, so any whitespace left in a
            // web target is the fold the header arrived on. Yahoo Japan
            // sends unsubscribe links long enough to wrap, and joining
            // the halves with the fold's space in place produces a URL
            // that 404s — one that looks right in a log.
            Some(Scheme::Web) => found.http.push(without_whitespace(target)),
            // Not for `mailto:`, where the space is content: one sender
            // ships `?body=Hi! I am requesting to be removed…`, and
            // squeezing that gives the recipient a wall of one word.
            Some(Scheme::Mail) => found.mailto.push(target.to_string()),
            None => {}
        }
    }
    if found.is_empty() {
        return None;
    }
    // A one-click claim with nowhere to post it is not one-click.
    found.one_click = found.one_click && found.one_click_url().is_some();
    Some(found)
}

enum Scheme {
    Web,
    Mail,
}

fn without_whitespace(target: &str) -> String {
    if !target.bytes().any(|b| b.is_ascii_whitespace()) {
        return target.to_string();
    }
    target
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect()
}

fn scheme_of(target: &str) -> Option<Scheme> {
    let lower_prefix =
        |p: &str| target.len() > p.len() && target[..p.len()].eq_ignore_ascii_case(p);
    if lower_prefix("https:") || lower_prefix("http:") {
        return Some(Scheme::Web);
    }
    if lower_prefix("mailto:") {
        return Some(Scheme::Mail);
    }
    None
}

/// `List-Unsubscribe-Post: List-Unsubscribe=One-Click`.
///
/// Looks for the token, not the whole line: the six senders that repeat
/// the field name in the value, and the one that leaves a stray colon
/// in front of it, mean the same thing as the 12,315 that get it right.
fn is_one_click(post: &str) -> bool {
    post.to_ascii_lowercase().contains("one-click")
}

/// Split the header into candidate targets.
///
/// Bracketed groups when there are any, and the whole trimmed value
/// when there are none — commas are only a separator between brackets,
/// which is what keeps a URL containing one intact.
fn targets(value: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let start = i + 1;
        // Ends at `>`, or at the next `<` — one sender in 10,988 omits
        // the closing bracket on its `mailto:`, and reading to the far
        // `>` swallows the https target after it into an address that
        // could never be sent to.
        match bytes[start..].iter().position(|&b| b == b'>' || b == b'<') {
            Some(offset) => {
                let end = start + offset;
                let inner = value[start..end].trim().trim_end_matches(',').trim();
                if !inner.is_empty() {
                    out.push(inner);
                }
                // Step onto the `<`, not past it: it opens the group
                // the missing `>` was hiding.
                if bytes[end] == b'<' {
                    i = end;
                } else {
                    i = end + 1;
                }
            }
            // An opening bracket with no close: the rest of the value
            // is the target, malformed but recoverable.
            None => {
                let inner = value[start..].trim();
                if !inner.is_empty() {
                    out.push(inner);
                }
                break;
            }
        }
    }
    if out.is_empty() {
        let bare = value.trim();
        if !bare.is_empty() {
            out.push(bare);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_ordinary_two_target_header() {
        let parsed = parse_unsubscribe(
            "<https://example.com/u?t=abc>, <mailto:unsub@example.com>",
            Some("List-Unsubscribe=One-Click"),
        )
        .expect("two targets");
        assert_eq!(parsed.http, vec!["https://example.com/u?t=abc"]);
        assert_eq!(parsed.mailto, vec!["mailto:unsub@example.com"]);
        assert!(parsed.one_click);
        assert_eq!(parsed.one_click_url(), Some("https://example.com/u?t=abc"));
    }

    /// 221 of 13,441. Microsoft and Slack both send it this way.
    #[test]
    fn accepts_a_bare_url_without_brackets() {
        let parsed = parse_unsubscribe(
            "https://account.microsoft.com/profile/unsubscribe?CTID=0&K=9a49",
            None,
        )
        .expect("bare url");
        assert_eq!(
            parsed.http,
            vec!["https://account.microsoft.com/profile/unsubscribe?CTID=0&K=9a49"]
        );
        assert!(!parsed.one_click);
    }

    /// 19 of them. Splitting the value on commas cuts these in half.
    #[test]
    fn keeps_a_comma_inside_the_url() {
        let raw = "<https://esp.example/u?ids=1,2,3&k=z>";
        let parsed = parse_unsubscribe(raw, None).expect("one target");
        assert_eq!(parsed.http, vec!["https://esp.example/u?ids=1,2,3&k=z"]);
    }

    #[test]
    fn tolerates_the_spellings_senders_actually_use() {
        for post in [
            "List-Unsubscribe=One-Click",
            "List-Unsubscribe=One-click",
            "List-Unsubscribe-Post=One-Click",
            ": List-Unsubscribe=One-Click",
            "list-unsubscribe=one-click",
        ] {
            let parsed = parse_unsubscribe("<https://x.example/u>", Some(post))
                .unwrap_or_else(|| panic!("{post}"));
            assert!(parsed.one_click, "{post}");
        }
    }

    #[test]
    fn an_unexpanded_merge_tag_is_not_a_link() {
        let raw = "<%%=concat(cloudpagesurl(1135,'jobid',jobid),'&')=%%?jwt=eyJh>";
        assert_eq!(
            parse_unsubscribe(raw, Some("List-Unsubscribe=One-Click")),
            None
        );
    }

    #[test]
    fn plain_http_is_offered_but_never_posted_to() {
        let parsed = parse_unsubscribe(
            "<http://esp.example/u?t=abc>",
            Some("List-Unsubscribe=One-Click"),
        )
        .expect("one target");
        assert_eq!(parsed.http.len(), 1, "the link is still worth showing");
        assert!(
            !parsed.one_click,
            "an opaque token must not go out in the clear"
        );
        assert_eq!(parsed.one_click_url(), None);
    }

    #[test]
    fn a_one_click_claim_with_only_mailto_is_not_one_click() {
        let parsed =
            parse_unsubscribe("<mailto:u@example.com>", Some("List-Unsubscribe=One-Click"))
                .expect("one target");
        assert!(!parsed.one_click);
        assert_eq!(parsed.one_click_url(), None);
    }

    #[test]
    fn an_empty_or_useless_header_is_nothing() {
        assert_eq!(parse_unsubscribe("", None), None);
        assert_eq!(parse_unsubscribe("   ", None), None);
        assert_eq!(parse_unsubscribe("<>", None), None);
        assert_eq!(parse_unsubscribe("NO", None), None);
    }

    #[test]
    fn an_unclosed_bracket_still_yields_its_target() {
        let parsed = parse_unsubscribe("<https://x.example/u", None).expect("recovered");
        assert_eq!(parsed.http, vec!["https://x.example/u"]);
    }

    #[test]
    fn keeps_the_order_the_header_gave() {
        let parsed = parse_unsubscribe(
            "<mailto:a@x.example>, <https://x.example/1>, <https://x.example/2>",
            None,
        )
        .expect("three");
        assert_eq!(
            parsed.http,
            vec!["https://x.example/1", "https://x.example/2"]
        );
        assert_eq!(parsed.mailto, vec!["mailto:a@x.example"]);
    }

    /// A long link wraps, and unfolding leaves the fold's space behind.
    /// Yahoo Japan's opt-in links are past 80 columns and arrive this
    /// way; joined with the space still in them they 404.
    #[test]
    fn a_folded_web_target_is_rejoined() {
        let parsed = parse_unsubscribe(
            "<https://mail-unsubscribe.yahooapis.jp/v1/optin/4bda5f716929 d2e7145a51%26221628>",
            None,
        )
        .expect("one target");
        assert_eq!(
            parsed.http,
            vec!["https://mail-unsubscribe.yahooapis.jp/v1/optin/4bda5f716929d2e7145a51%26221628"]
        );
    }

    /// The same squeeze applied to a `mailto:` would destroy the body
    /// text one sender pre-fills, which is the point of that target.
    #[test]
    fn a_mailto_keeps_its_spaces() {
        let raw = "<mailto:u-DC01@esp.example?subject=Unsubscribe&body=Hi! Please remove me.>";
        let parsed = parse_unsubscribe(raw, None).expect("one target");
        assert_eq!(
            parsed.mailto,
            vec!["mailto:u-DC01@esp.example?subject=Unsubscribe&body=Hi! Please remove me."]
        );
    }

    /// One header in 10,988 — LinkedIn's — leaves the `mailto:` group
    /// unterminated. Reading to the next `>` makes one target out of
    /// two: an address nothing could be sent to, with the working https
    /// link buried inside it.
    #[test]
    fn an_unterminated_group_does_not_swallow_the_next() {
        let raw = "<mailto:bounce@em.example?subject=unsubscribe%3Cabc%3E, \
                   <https://www.example.com/psettings/email-unsubscribe?loid=AQF>";
        let parsed = parse_unsubscribe(raw, None).expect("two targets");
        assert_eq!(
            parsed.mailto,
            vec!["mailto:bounce@em.example?subject=unsubscribe%3Cabc%3E"]
        );
        assert_eq!(
            parsed.http,
            vec!["https://www.example.com/psettings/email-unsubscribe?loid=AQF"]
        );
    }

    /// The scheme match is case-insensitive because the header is not
    /// normalised anywhere before it gets here.
    #[test]
    fn scheme_case_does_not_matter() {
        let parsed =
            parse_unsubscribe("<HTTPS://X.example/u>, <MailTo:a@x.example>", None).expect("two");
        assert_eq!(parsed.http.len(), 1);
        assert_eq!(parsed.mailto.len(), 1);
        // ...but a case-shifted scheme is still a valid post target.
        let posted = parse_unsubscribe("<HTTPS://X.example/u>", Some("One-Click")).expect("one");
        assert_eq!(posted.one_click_url(), Some("HTTPS://X.example/u"));
    }
}
