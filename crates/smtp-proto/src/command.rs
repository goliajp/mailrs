#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMechanism {
    Plain,
    Login,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command<'a> {
    Ehlo(&'a str),
    Helo(&'a str),
    MailFrom {
        path: ReversePath<'a>,
        params: Vec<Param<'a>>,
    },
    RcptTo {
        path: ForwardPath<'a>,
        params: Vec<Param<'a>>,
    },
    Data,
    Rset,
    Quit,
    Noop(Option<&'a str>),
    Vrfy(&'a str),
    Help(Option<&'a str>),
    StartTls,
    Auth {
        mechanism: AuthMechanism,
        initial_response: Option<&'a str>,
    },
    AuthResponse(&'a str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReversePath<'a> {
    Null,
    Path(&'a str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardPath<'a> {
    Postmaster,
    Path(&'a str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param<'a> {
    pub key: &'a str,
    pub value: &'a str,
}
