use std::sync::Arc;

use crate::domain_store::DomainStore;
use crate::inbound::auth_guard::{AuthCheck, AuthGuard};
use mailrs_sieve::compile_sieve;
use crate::users::UserStore;

/// ManageSieve session state (RFC 5804)
enum SieveState {
    NotAuthenticated,
    Authenticated { username: String },
}

/// ManageSieve protocol session handler
pub struct ManageSieveSession {
    domain_store: Option<Arc<DomainStore>>,
    users: Arc<UserStore>,
    auth_guard: Option<Arc<AuthGuard>>,
    peer_addr: Option<std::net::IpAddr>,
    state: SieveState,
    ldap_config: Option<Arc<crate::ldap_auth::LdapConfig>>,
}

impl ManageSieveSession {
    pub fn new(users: Arc<UserStore>) -> Self {
        Self {
            domain_store: None,
            users,
            auth_guard: None,
            peer_addr: None,
            state: SieveState::NotAuthenticated,
            ldap_config: None,
        }
    }

    pub fn with_domain_store(mut self, ds: Arc<DomainStore>) -> Self {
        self.domain_store = Some(ds);
        self
    }

    pub fn with_auth_guard(mut self, guard: Arc<AuthGuard>, addr: std::net::IpAddr) -> Self {
        self.auth_guard = Some(guard);
        self.peer_addr = Some(addr);
        self
    }

    pub fn with_ldap_config(mut self, config: Arc<crate::ldap_auth::LdapConfig>) -> Self {
        self.ldap_config = Some(config);
        self
    }

    pub fn greeting(&self) -> String {
        "\"IMPLEMENTATION\" \"mailrs\"\r\n\
         \"SIEVE\" \"fileinto reject vacation\"\r\n\
         OK\r\n"
            .to_string()
    }

    /// handle a single ManageSieve command line, return response(s)
    pub async fn handle_line(&mut self, line: &str) -> Vec<String> {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            return vec![];
        }

        let (cmd, args) = match trimmed.split_once(' ') {
            Some((c, a)) => (c.to_uppercase(), a.to_string()),
            None => (trimmed.to_uppercase(), String::new()),
        };

        match cmd.as_str() {
            "CAPABILITY" => self.handle_capability(),
            "AUTHENTICATE" => self.handle_authenticate(&args).await,
            "LISTSCRIPTS" => self.handle_listscripts().await,
            "GETSCRIPT" => self.handle_getscript(&args).await,
            "PUTSCRIPT" => self.handle_putscript(&args).await,
            "DELETESCRIPT" => self.handle_deletescript(&args).await,
            "SETACTIVE" => self.handle_setactive(&args),
            "LOGOUT" => self.handle_logout(),
            "HAVESPACE" => {
                // always report enough space
                vec!["OK\r\n".into()]
            }
            _ => vec![format!("NO \"unknown command\"\r\n")],
        }
    }

    fn handle_capability(&self) -> Vec<String> {
        vec![
            "\"IMPLEMENTATION\" \"mailrs\"\r\n".into(),
            "\"SIEVE\" \"fileinto reject vacation\"\r\n".into(),
            "\"SASL\" \"PLAIN\"\r\n".into(),
            "OK\r\n".into(),
        ]
    }

    async fn handle_authenticate(&mut self, args: &str) -> Vec<String> {
        if matches!(self.state, SieveState::Authenticated { .. }) {
            return vec!["NO \"already authenticated\"\r\n".into()];
        }

        // expect: "PLAIN" <base64>
        let parts: Vec<&str> = args.splitn(2, ' ').collect();
        if parts.len() < 2 {
            return vec!["NO \"missing credentials\"\r\n".into()];
        }

        let mechanism = unquote(parts[0].trim());
        if mechanism.to_uppercase() != "PLAIN" {
            return vec!["NO \"unsupported mechanism\"\r\n".into()];
        }

        let b64 = unquote(parts[1].trim());
        let decoded = match base64_decode(&b64) {
            Some(d) => d,
            None => return vec!["NO \"invalid base64\"\r\n".into()],
        };

        // PLAIN format: \0username\0password
        let parts: Vec<&[u8]> = decoded.splitn(3, |&b| b == 0).collect();
        if parts.len() < 3 {
            return vec!["NO \"invalid PLAIN format\"\r\n".into()];
        }

        let username = String::from_utf8_lossy(parts[1]).to_string();
        let password = String::from_utf8_lossy(parts[2]).to_string();

        if username.is_empty() || password.is_empty() {
            return vec!["NO \"empty username or password\"\r\n".into()];
        }

        // check auth guard
        if let (Some(guard), Some(ip)) = (&self.auth_guard, self.peer_addr)
            && let AuthCheck::LockedOut { .. } = guard.check(ip, &username) {
                return vec!["NO \"too many failures, try later\"\r\n".into()];
            }

        // authenticate: try domain store first, then users.toml, then LDAP
        let authenticated = if let Some(ref ds) = self.domain_store {
            match ds.get_account_with_hash(&username).await {
                Ok(Some((account, hash))) => {
                    if !account.active {
                        false
                    } else if hash.is_empty() {
                        // accounts with no password hash cannot log in
                        if let Some(ref ldap) = self.ldap_config {
                            ldap.authenticate(&username, &password).await
                        } else {
                            false
                        }
                    } else if hash.starts_with("$argon2") {
                        let valid = UserStore::verify_hash(&password, &hash);
                        if valid {
                            true
                        } else if let Some(ref ldap) = self.ldap_config {
                            ldap.authenticate(&username, &password).await
                        } else {
                            false
                        }
                    } else if hash == password {
                        true
                    } else if let Some(ref ldap) = self.ldap_config {
                        ldap.authenticate(&username, &password).await
                    } else {
                        false
                    }
                }
                _ => {
                    if self.users.verify(&username, &password) {
                        true
                    } else if let Some(ref ldap) = self.ldap_config {
                        ldap.authenticate(&username, &password).await
                    } else {
                        false
                    }
                }
            }
        } else if self.users.verify(&username, &password) {
            true
        } else if let Some(ref ldap) = self.ldap_config {
            ldap.authenticate(&username, &password).await
        } else {
            false
        };

        if !authenticated {
            if let (Some(guard), Some(ip)) = (&self.auth_guard, self.peer_addr) {
                guard.record_failure(ip, &username);
            }
            return vec!["NO \"authentication failed\"\r\n".into()];
        }

        if let (Some(guard), Some(ip)) = (&self.auth_guard, self.peer_addr) {
            guard.record_success(ip, &username);
        }

        self.state = SieveState::Authenticated {
            username: username.clone(),
        };
        vec![format!("OK \"authenticated as {username}\"\r\n")]
    }

    async fn handle_listscripts(&self) -> Vec<String> {
        let SieveState::Authenticated { ref username } = self.state else {
            return vec!["NO \"not authenticated\"\r\n".into()];
        };

        let Some(ref ds) = self.domain_store else {
            return vec![
                "\"default\"\r\n".into(),
                "OK\r\n".into(),
            ];
        };

        match ds.get_sieve_script(username).await {
            Ok(Some(_)) => vec![
                "\"default\" ACTIVE\r\n".into(),
                "OK\r\n".into(),
            ],
            Ok(None) => vec!["OK\r\n".into()],
            Err(e) => vec![format!("NO \"{e}\"\r\n")],
        }
    }

    async fn handle_getscript(&self, args: &str) -> Vec<String> {
        let SieveState::Authenticated { ref username } = self.state else {
            return vec!["NO \"not authenticated\"\r\n".into()];
        };

        let name = unquote(args.trim());
        if name.is_empty() {
            return vec!["NO \"script name required\"\r\n".into()];
        }

        let Some(ref ds) = self.domain_store else {
            return vec!["NO \"no storage backend\"\r\n".into()];
        };

        match ds.get_sieve_script(username).await {
            Ok(Some(script)) => {
                let size = script.len();
                vec![
                    format!("{{{size}}}\r\n"),
                    format!("{script}\r\n"),
                    "OK\r\n".into(),
                ]
            }
            Ok(None) => vec!["NO \"script not found\"\r\n".into()],
            Err(e) => vec![format!("NO \"{e}\"\r\n")],
        }
    }

    async fn handle_putscript(&mut self, args: &str) -> Vec<String> {
        let SieveState::Authenticated { ref username } = self.state else {
            return vec!["NO \"not authenticated\"\r\n".into()];
        };

        // parse: "name" {size}\r\ncontent
        // in simplified form we expect: "name" {size}\r\ncontent all in one line
        // or the content may follow on subsequent lines — but our line-based
        // reader will concatenate literal data. for now, parse inline.
        let args = args.trim();

        // extract script name
        let (name, rest) = if let Some(stripped) = args.strip_prefix('"') {
            let end = stripped.find('"').unwrap_or(args.len() - 1);
            let n = &stripped[..end];
            (n.to_string(), stripped[end + 1..].trim())
        } else {
            match args.split_once(' ') {
                Some((n, r)) => (n.to_string(), r.trim()),
                None => return vec!["NO \"missing script content\"\r\n".into()],
            }
        };

        if name.is_empty() {
            return vec!["NO \"script name required\"\r\n".into()];
        }

        // extract content from literal {size}\r\ncontent
        let content = if rest.starts_with('{') {
            // find closing brace
            let brace_end = rest.find('}').unwrap_or(0);
            // content follows after \r\n or immediately
            let after_brace = &rest[brace_end + 1..];
            let content = after_brace.trim_start_matches("\r\n").trim_start_matches('\n');
            content.to_string()
        } else {
            rest.to_string()
        };

        if content.is_empty() {
            return vec!["NO \"empty script\"\r\n".into()];
        }

        // validate sieve script
        if let Err(e) = compile_sieve(&content) {
            return vec![format!("NO \"compilation failed: {e}\"\r\n")];
        }

        let Some(ref ds) = self.domain_store else {
            return vec!["NO \"no storage backend\"\r\n".into()];
        };

        let username = username.clone();
        let now = chrono::Utc::now().timestamp();
        match ds.set_sieve_script(&username, &content, now).await {
            Ok(()) => vec![format!("OK \"script \\\"{name}\\\" saved\"\r\n")],
            Err(e) => vec![format!("NO \"{e}\"\r\n")],
        }
    }

    async fn handle_deletescript(&mut self, args: &str) -> Vec<String> {
        let SieveState::Authenticated { ref username } = self.state else {
            return vec!["NO \"not authenticated\"\r\n".into()];
        };

        let name = unquote(args.trim());
        if name.is_empty() {
            return vec!["NO \"script name required\"\r\n".into()];
        }

        let Some(ref ds) = self.domain_store else {
            return vec!["NO \"no storage backend\"\r\n".into()];
        };

        let username = username.clone();
        match ds.delete_sieve_script(&username).await {
            Ok(true) => vec![format!("OK \"script \\\"{name}\\\" deleted\"\r\n")],
            Ok(false) => vec!["NO \"script not found\"\r\n".into()],
            Err(e) => vec![format!("NO \"{e}\"\r\n")],
        }
    }

    fn handle_setactive(&self, args: &str) -> Vec<String> {
        if !matches!(self.state, SieveState::Authenticated { .. }) {
            return vec!["NO \"not authenticated\"\r\n".into()];
        }

        let name = unquote(args.trim());
        if name.is_empty() {
            // deactivate: no-op since we only support one script
            return vec!["OK \"no active script\"\r\n".into()];
        }

        // we only support one script per account, so setting active is a no-op
        vec!["OK\r\n".into()]
    }

    fn handle_logout(&mut self) -> Vec<String> {
        self.state = SieveState::NotAuthenticated;
        vec!["OK \"Bye\"\r\n".into()]
    }

    /// returns true if the session should be closed
    pub fn should_close(&self, responses: &[String]) -> bool {
        responses.last().is_some_and(|r| r.contains("Bye"))
    }
}

/// remove surrounding double quotes from a string
fn unquote(s: &str) -> String {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// decode base64 string
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(input).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session() -> ManageSieveSession {
        ManageSieveSession::new(Arc::new(UserStore::empty()))
    }

    #[test]
    fn greeting_contains_implementation() {
        let session = make_session();
        let greeting = session.greeting();
        assert!(greeting.contains("IMPLEMENTATION"));
        assert!(greeting.contains("mailrs"));
    }

    #[tokio::test]
    async fn capability_response() {
        let mut session = make_session();
        let resp = session.handle_line("CAPABILITY").await;
        let joined = resp.join("");
        assert!(joined.contains("SIEVE"));
        assert!(joined.contains("SASL"));
        assert!(joined.contains("OK"));
    }

    #[tokio::test]
    async fn authenticate_fails_without_credentials() {
        let mut session = make_session();
        let resp = session.handle_line("AUTHENTICATE \"PLAIN\"").await;
        assert!(resp[0].starts_with("NO"));
    }

    #[tokio::test]
    async fn authenticate_fails_bad_mechanism() {
        let mut session = make_session();
        let resp = session.handle_line("AUTHENTICATE \"CRAM-MD5\" dGVzdA==").await;
        assert!(resp[0].starts_with("NO"));
        assert!(resp[0].contains("unsupported"));
    }

    #[tokio::test]
    async fn authenticate_fails_invalid_base64() {
        let mut session = make_session();
        let resp = session.handle_line("AUTHENTICATE \"PLAIN\" !!!invalid!!!").await;
        assert!(resp[0].starts_with("NO"));
    }

    #[tokio::test]
    async fn authenticate_fails_bad_credentials() {
        let users = Arc::new(UserStore::from_plain_passwords(vec![
            ("alice@example.com".into(), "secret".into()),
        ]));
        let mut session = ManageSieveSession::new(users);
        // encode \0alice@example.com\0wrong
        let cred = base64_encode(b"\0alice@example.com\0wrong");
        let resp = session
            .handle_line(&format!("AUTHENTICATE \"PLAIN\" \"{cred}\""))
            .await;
        assert!(resp[0].starts_with("NO"));
    }

    #[tokio::test]
    async fn authenticate_succeeds() {
        let users = Arc::new(UserStore::from_plain_passwords(vec![
            ("alice@example.com".into(), "secret".into()),
        ]));
        let mut session = ManageSieveSession::new(users);
        let cred = base64_encode(b"\0alice@example.com\0secret");
        let resp = session
            .handle_line(&format!("AUTHENTICATE \"PLAIN\" \"{cred}\""))
            .await;
        assert!(resp[0].starts_with("OK"));
        assert!(resp[0].contains("authenticated"));
    }

    #[tokio::test]
    async fn listscripts_requires_auth() {
        let mut session = make_session();
        let resp = session.handle_line("LISTSCRIPTS").await;
        assert!(resp[0].starts_with("NO"));
    }

    #[tokio::test]
    async fn getscript_requires_auth() {
        let mut session = make_session();
        let resp = session.handle_line("GETSCRIPT \"default\"").await;
        assert!(resp[0].starts_with("NO"));
    }

    #[tokio::test]
    async fn logout_response() {
        let mut session = make_session();
        let resp = session.handle_line("LOGOUT").await;
        assert!(resp[0].contains("Bye"));
        assert!(session.should_close(&resp));
    }

    #[tokio::test]
    async fn unknown_command() {
        let mut session = make_session();
        let resp = session.handle_line("FOOBAR").await;
        assert!(resp[0].starts_with("NO"));
    }

    #[tokio::test]
    async fn setactive_requires_auth() {
        let mut session = make_session();
        let resp = session.handle_line("SETACTIVE \"default\"").await;
        assert!(resp[0].starts_with("NO"));
    }

    #[tokio::test]
    async fn havespace_always_ok() {
        let mut session = make_session();
        let resp = session.handle_line("HAVESPACE \"test\" 1024").await;
        assert_eq!(resp[0], "OK\r\n");
    }

    #[test]
    fn unquote_removes_quotes() {
        assert_eq!(unquote("\"hello\""), "hello");
        assert_eq!(unquote("hello"), "hello");
        assert_eq!(unquote("\"\""), "");
        assert_eq!(unquote("\""), "\"");
    }

    #[test]
    fn base64_decode_valid() {
        let decoded = base64_decode("dGVzdA==");
        assert_eq!(decoded, Some(b"test".to_vec()));
    }

    #[test]
    fn base64_decode_invalid() {
        assert!(base64_decode("!!!").is_none());
    }

    fn base64_encode(input: &[u8]) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(input)
    }
}
