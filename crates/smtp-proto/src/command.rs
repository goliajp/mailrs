//! Typed SMTP command representation.

/// SASL authentication mechanism advertised by the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMechanism {
    /// `AUTH PLAIN` — RFC 4616.
    Plain,
    /// `AUTH LOGIN` — non-standard but widely supported.
    Login,
}

/// A parsed SMTP command, borrowed from the input line.
///
/// Lifetimes match the input slice given to [`parse_command`], so commands
/// must be consumed before the input buffer is reused.
///
/// [`parse_command`]: crate::parse_command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command<'a> {
    /// `EHLO <domain>` — start an ESMTP session.
    Ehlo(&'a str),
    /// `HELO <domain>` — start a legacy SMTP session.
    Helo(&'a str),
    /// `MAIL FROM:<reverse-path> [params]` — open a mail transaction.
    MailFrom {
        /// Reverse path (envelope sender).
        path: ReversePath<'a>,
        /// ESMTP parameters (SIZE, AUTH, BODY, etc).
        params: Vec<Param<'a>>,
    },
    /// `RCPT TO:<forward-path> [params]` — add a recipient.
    RcptTo {
        /// Forward path (envelope recipient).
        path: ForwardPath<'a>,
        /// ESMTP parameters (NOTIFY, ORCPT, etc).
        params: Vec<Param<'a>>,
    },
    /// `DATA` — begin message body input.
    Data,
    /// `RSET` — reset the mail transaction.
    Rset,
    /// `QUIT` — close the connection.
    Quit,
    /// `NOOP [string]` — no-op.
    Noop(Option<&'a str>),
    /// `VRFY <user>` — verify a user.
    Vrfy(&'a str),
    /// `HELP [topic]` — request help.
    Help(Option<&'a str>),
    /// `STARTTLS` — upgrade the connection to TLS (RFC 3207).
    StartTls,
    /// `AUTH <mechanism> [initial-response]` — start SASL authentication.
    Auth {
        /// SASL mechanism the client wants to use.
        mechanism: AuthMechanism,
        /// Optional initial client response (base64-encoded SASL payload).
        initial_response: Option<&'a str>,
    },
    /// Continuation line during an in-progress AUTH challenge.
    AuthResponse(&'a str),
}

/// Reverse path (the `MAIL FROM:<...>` envelope sender).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReversePath<'a> {
    /// Empty reverse path (`<>`) — null sender, used for bounces.
    Null,
    /// Concrete sender address.
    Path(&'a str),
}

/// Forward path (the `RCPT TO:<...>` envelope recipient).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardPath<'a> {
    /// Special `<Postmaster>` recipient required by RFC 5321 § 4.1.1.3.
    Postmaster,
    /// Concrete recipient address.
    Path(&'a str),
}

/// A `key=value` (or bare `key`) ESMTP parameter, e.g. `SIZE=12345` or
/// `BODY=8BITMIME`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param<'a> {
    /// Parameter name (uppercase by convention, but match case-insensitively).
    pub key: &'a str,
    /// Parameter value, or empty string for bare parameters.
    pub value: &'a str,
}
