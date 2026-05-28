//! Differential-test corpus — message fixtures + per-slice
//! corpus functions. Lives in `tests/common/` so the corpus and
//! the test-driver file each stay under the file-size limit.

use super::CorpusRow;

// --- Message fixtures (module-level consts to keep corpus fns small) ---

pub(crate) const MSG_SPAM: &[u8] = b"\
From: Alice <alice@example.com>\r\n\
To: bob@dest.com\r\n\
Subject: spam offer\r\n\
\r\n\
hello\r\n";

pub(crate) const MSG_CLEAN: &[u8] = b"\
From: Bob <bob@trusted.com>\r\n\
To: alice@example.com\r\n\
Subject: meeting tomorrow\r\n\
\r\n\
agenda attached\r\n";

// Long header to exercise :contains over 150+ char header values.
pub(crate) const MSG_LONGHDR: &[u8] = b"\
From: Alice <alice@example.com>\r\n\
To: bob@dest.com\r\n\
Subject: very long subject line that goes on for many characters including the NEEDLE_TOKEN word and continues with more filler text after that point to ensure it exceeds one hundred and fifty chars\r\n\
\r\n\
body\r\n";

// Folded Subject (RFC 5322 §2.2.3). The unfolder must collapse
// `\r\n ` / `\r\n\t` so `:contains` sees a single logical line.
pub(crate) const MSG_FOLDED: &[u8] = b"\
From: Alice <alice@example.com>\r\n\
To: bob@dest.com\r\n\
Subject: starts here\r\n continues on next line\r\n\twith tab continuation too\r\n\
\r\n\
body\r\n";

// From with quoted display-name containing a comma — exercises
// address-extraction in `address :localpart`.
pub(crate) const MSG_QUOTEDNAME: &[u8] = b"\
From: \"Alice, Sr.\" <alice@example.com>\r\n\
To: bob@dest.com\r\n\
Subject: hello\r\n\
\r\n\
body\r\n";

// Multi-recipient To header + Cc — exercises address tests
// against multi-valued headers and multiple matching candidates.
pub(crate) const MSG_MULTI_RCPT: &[u8] = b"\
From: Alice <alice@example.com>\r\n\
To: bob@dest.com, carol@other.com, dave@third.com\r\n\
Cc: ed@cc.com\r\n\
Subject: team update\r\n\
\r\n\
body\r\n";

// Subject containing an escaped quote — round-trips through the
// header value verbatim so engines can match against `He said "hi"`.
pub(crate) const MSG_QUOTED_SUBJECT: &[u8] = b"\
From: Alice <alice@example.com>\r\n\
To: bob@dest.com\r\n\
Subject: He said \"hi\"\r\n\
\r\n\
body\r\n";

mod slice12;
mod slice3;
mod slice4_a;
mod slice4_b;
mod slice4_c;
mod slice4_d;
mod slice4_e;
mod slice4_f;
mod slice4_g;
mod slice5_a;

/// Combined corpus driven by the diff test.
pub fn corpus() -> Vec<CorpusRow> {
    let mut all = slice12::corpus();
    all.extend(slice3::corpus());
    all.extend(slice4_a::corpus());
    all.extend(slice4_b::corpus());
    all.extend(slice4_c::corpus());
    all.extend(slice4_d::corpus());
    all.extend(slice4_e::corpus());
    all.extend(slice4_f::corpus());
    all.extend(slice4_g::corpus());
    all.extend(slice5_a::corpus());
    all
}
