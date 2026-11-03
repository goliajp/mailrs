//! Forwarding loops: a message this server forwards carries a
//! `Delivered-To:` line for each address that forwarded it. One that comes
//! back naming the address about to forward it again has been round a
//! loop; one with more lines than any honest chain of forwards has is
//! treated the same, whoever wrote them.

/// This many `Delivered-To:` lines and a message is not forwarded again.
pub(crate) const MAX_HOPS: usize = 10;

/// Why `body` must not be forwarded from `from`, if it must not.
pub(crate) fn refused(body: &[u8], from: &str) -> Option<String> {
    let delivered_to = delivered_to(body);
    if delivered_to
        .iter()
        .any(|a| mailrs_rfc5322::addr_spec(a).eq_ignore_ascii_case(from.trim()))
    {
        return Some(format!("forwarding loop: already forwarded from {from}"));
    }
    if delivered_to.len() >= MAX_HOPS {
        return Some(format!(
            "forwarding loop: {} hops, the most is {MAX_HOPS}",
            delivered_to.len()
        ));
    }
    None
}

/// `body` with a `Delivered-To:` line for `from` in front.
pub(crate) fn marked(body: &[u8], from: &str) -> Vec<u8> {
    let from: String = from.chars().filter(|c| !c.is_control()).collect();
    let mut out = format!("Delivered-To: {from}\r\n").into_bytes();
    out.extend_from_slice(body);
    out
}

/// The values of every `Delivered-To:` header in `body`'s head.
fn delivered_to(body: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(&body[..body.len().min(64 * 1024)]);
    let head_end = text
        .find("\r\n\r\n")
        .or_else(|| text.find("\n\n"))
        .unwrap_or(text.len());
    text[..head_end]
        .split('\n')
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            match name.eq_ignore_ascii_case("delivered-to") {
                true => Some(value.trim().to_string()),
                false => None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIL: &[u8] = b"From: carol@outside.example\r\nTo: a@example.test\r\n\
Subject: hi\r\n\r\nDelivered-To: a@example.test\r\n";

    /// a@ forwards to b@ elsewhere, which forwards back to a@: the copy
    /// that returns names a@ and goes no further.
    #[test]
    fn a_message_back_at_an_address_it_was_forwarded_from_stops() {
        assert_eq!(
            refused(MAIL, "a@example.test"),
            None,
            "a body line is not a header"
        );
        let once = marked(MAIL, "a@example.test");
        assert!(once.starts_with(b"Delivered-To: a@example.test\r\nFrom:"));
        let back = [b"Received: from mx.elsewhere\r\n".as_slice(), &once].concat();
        assert!(refused(&back, "A@Example.test").is_some());
        assert_eq!(refused(&back, "b@example.test"), None);
    }

    #[test]
    fn too_many_hops_stop_whoever_wrote_them() {
        let mut body = MAIL.to_vec();
        for i in 0..MAX_HOPS - 1 {
            body = marked(&body, &format!("hop{i}@elsewhere.test"));
        }
        assert_eq!(refused(&body, "a@example.test"), None);
        body = marked(&body, "last@elsewhere.test");
        assert!(refused(&body, "a@example.test").is_some());
    }

    #[test]
    fn the_mark_is_one_line() {
        let m = marked(b"\r\nx", "a@x\r\nBcc: e@x");
        assert!(m.starts_with(b"Delivered-To: a@xBcc: e@x\r\n\r\nx"));
    }
}
