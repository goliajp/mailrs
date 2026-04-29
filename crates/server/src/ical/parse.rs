//! RFC 5545 §3.1 text → AST.
//!
//! Hand-rolled byte-by-byte tokenizer + state machine. Handles:
//! - line folding / unfolding (CRLF + leading whitespace continuation)
//! - property line `key[;param=val[,val]*]:value` with quoted parameter values
//! - text escapes — left raw at the AST layer; [`super::semantics`] decides
//!   which fields are TEXT type and unescapes accordingly
//! - component nesting via BEGIN / END pairing
//!
//! No parser combinator deps. Style aligned with `smtp-proto::parse`.

use super::{IcalError, RawComponent, RawProperty};

/// Parse a complete VCALENDAR document into a raw component tree.
///
/// The returned [`RawComponent`] is always named `VCALENDAR`; its children
/// include 0..N `VEVENT` plus optional `VTIMEZONE` blocks.
pub fn parse_calendar(input: &str) -> Result<RawComponent, IcalError> {
    let logical_lines = unfold(input);

    // Component stack: top of stack is the currently-open component.
    let mut stack: Vec<RawComponent> = Vec::new();

    for line in logical_lines {
        if line.is_empty() {
            continue;
        }
        let prop = parse_property_line(&line)?;

        if prop.name.eq_ignore_ascii_case("BEGIN") {
            // Start a new component named after the value (e.g. VEVENT).
            stack.push(RawComponent {
                name: prop.value,
                properties: Vec::new(),
                children: Vec::new(),
            });
        } else if prop.name.eq_ignore_ascii_case("END") {
            let finished = stack.pop().ok_or_else(|| {
                IcalError::InvalidSyntax(format!(
                    "END:{} without matching BEGIN",
                    prop.value
                ))
            })?;
            if !finished.name.eq_ignore_ascii_case(&prop.value) {
                return Err(IcalError::InvalidSyntax(format!(
                    "END:{} closes BEGIN:{}",
                    prop.value, finished.name
                )));
            }
            match stack.last_mut() {
                Some(parent) => parent.children.push(finished),
                None => {
                    // Closed the outermost component. It must be VCALENDAR.
                    if !finished.name.eq_ignore_ascii_case("VCALENDAR") {
                        return Err(IcalError::InvalidSyntax(format!(
                            "outermost component is {}, expected VCALENDAR",
                            finished.name
                        )));
                    }
                    return Ok(finished);
                }
            }
        } else {
            // Regular property — attach to current component.
            let current = stack.last_mut().ok_or_else(|| {
                IcalError::InvalidSyntax(format!(
                    "property {} appears outside any component",
                    prop.name
                ))
            })?;
            current.properties.push(prop);
        }
    }

    if !stack.is_empty() {
        return Err(IcalError::InvalidSyntax(format!(
            "unclosed BEGIN:{}",
            stack.last().expect("non-empty").name
        )));
    }
    Err(IcalError::InvalidSyntax("no VCALENDAR component found".into()))
}

/// RFC 5545 §3.1 line unfolding: a CRLF (or LF for tolerance) followed by
/// whitespace (SPACE or HTAB) is a continuation; merge with the previous line.
///
/// Empty lines are preserved as-is so the caller can skip them. A trailing
/// newline does not produce a spurious empty entry.
fn unfold(input: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();

    // Iterate lines, tolerating both CRLF and LF terminators (some inbox
    // scrapes strip CR). RFC 5545 §3.1 mandates CRLF; we accept LF for
    // compatibility with hand-written test fixtures + lenient producers.
    for raw_line in split_lines(input) {
        if let Some(rest) = raw_line.strip_prefix(' ').or_else(|| raw_line.strip_prefix('\t')) {
            // Continuation line: append (without the leading WSP) to current.
            current.push_str(rest);
        } else {
            if !current.is_empty() || !out.is_empty() || !raw_line.is_empty() {
                // Flush previous logical line (if any) before starting new one.
                out.push(std::mem::take(&mut current));
            }
            current.push_str(raw_line);
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    // The first push when out was empty produced an empty entry only if the
    // first physical line was empty — which is fine; callers skip empties.
    out.into_iter().filter(|s| !s.is_empty()).collect()
}

/// Split text on CRLF / LF without allocating per line.
fn split_lines(input: &str) -> impl Iterator<Item = &str> {
    // `split_terminator("\n")` then trim a trailing '\r'.
    input.split_terminator('\n').map(|line| line.strip_suffix('\r').unwrap_or(line))
}

/// Parse a single (already-unfolded) property line.
///
/// Format: `name[;param=value[,value]*]*:value`
/// - The first unquoted `:` separates header from value.
/// - The header is split on unquoted `;`; the first piece is the property
///   name; the rest are `param=value` pairs (multiple values comma-separated,
///   collapsed here into a single string preserving the comma).
/// - Quoted parameter values (per §3.2) accept any non-CTL char including `;`
///   and `:` — that's the whole point of quoting.
fn parse_property_line(line: &str) -> Result<RawProperty, IcalError> {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut in_quotes = false;
    let mut value_start: Option<usize> = None;

    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            in_quotes = !in_quotes;
        } else if b == b':' && !in_quotes {
            value_start = Some(i);
            break;
        }
        i += 1;
    }

    let value_start = value_start.ok_or_else(|| {
        IcalError::InvalidSyntax(format!("property line missing ':' separator: {line}"))
    })?;

    let header = &line[..value_start];
    let value = &line[value_start + 1..];

    // Split header on unquoted ';'.
    let header_parts = split_unquoted_semicolons(header);
    let mut iter = header_parts.into_iter();
    let name = iter
        .next()
        .ok_or_else(|| IcalError::InvalidSyntax(format!("empty property header: {line}")))?
        .to_string();
    if name.is_empty() {
        return Err(IcalError::InvalidSyntax(format!(
            "empty property name in: {line}"
        )));
    }

    let mut params = Vec::new();
    for raw in iter {
        if let Some(eq) = raw.find('=') {
            let pname = raw[..eq].to_string();
            let pval = unquote_param(&raw[eq + 1..]);
            params.push((pname, pval));
        } else {
            // Bare flag-style parameter (uncommon but allowed in some impls).
            params.push((raw.to_string(), String::new()));
        }
    }

    Ok(RawProperty {
        name,
        params,
        value: value.to_string(),
    })
}

/// Split `header` on `;`, but ignore `;` inside double quotes.
fn split_unquoted_semicolons(header: &str) -> Vec<&str> {
    let bytes = header.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0;
    let mut in_quotes = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            in_quotes = !in_quotes;
        } else if b == b';' && !in_quotes {
            parts.push(&header[start..i]);
            start = i + 1;
        }
        i += 1;
    }
    parts.push(&header[start..]);
    parts
}

/// Strip surrounding double quotes from a parameter value if present.
///
/// RFC 5545 §3.2 keeps quoting strictly outermost (no escapes inside quotes).
fn unquote_param(s: &str) -> String {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    #[test]
    fn parses_single_property() {
        let p = parse_property_line("UID:abc-123").unwrap();
        assert_eq!(p.name, "UID");
        assert_eq!(p.value, "abc-123");
        assert!(p.params.is_empty());
    }

    #[test]
    fn parses_property_with_params() {
        let p =
            parse_property_line("ATTENDEE;CN=John Doe;PARTSTAT=ACCEPTED:mailto:j@example.com")
                .unwrap();
        assert_eq!(p.name, "ATTENDEE");
        assert_eq!(p.value, "mailto:j@example.com");
        assert_eq!(p.params.len(), 2);
        assert_eq!(p.params[0].0, "CN");
        assert_eq!(p.params[0].1, "John Doe");
        assert_eq!(p.params[1].0, "PARTSTAT");
        assert_eq!(p.params[1].1, "ACCEPTED");
    }

    #[test]
    fn handles_quoted_param_with_colon() {
        // RFC 5545 §3.2: param value with ':' must be quoted.
        let p = parse_property_line("X-FOO;BAR=\"weird:value;with-stuff\":real-value").unwrap();
        assert_eq!(p.name, "X-FOO");
        assert_eq!(p.value, "real-value");
        assert_eq!(p.params[0].1, "weird:value;with-stuff");
    }

    #[test]
    fn unfolds_continuation() {
        // RFC 5545 §3.1: a fold is CRLF + 1 WSP; that 1 WSP is stripped.
        // To keep a space inside the value across a fold, the producer must
        // emit 2 WSP. (This matches what real iCal producers do.)
        let lines = unfold("VERSION:2.0\r\nSUMMARY:long\r\n  part 2\r\n  part 3\r\n");
        assert_eq!(lines, vec!["VERSION:2.0", "SUMMARY:long part 2 part 3"]);
    }

    #[test]
    fn unfolds_with_tab_continuation() {
        let lines = unfold("FOO:bar\r\n\tcontinued\r\n");
        assert_eq!(lines, vec!["FOO:barcontinued"]);
    }
}
