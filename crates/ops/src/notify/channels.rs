//! The three phase-1 channels. None of them touches the local outbound
//! queue: the webhook and the push are plain HTTPS from this process, and
//! the relay is this process speaking SMTP to the deployment's relay.

use std::time::Duration;

use super::Message;
use crate::config::RelayConfig;

const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub enum Channel {
    Webhook { url: String, secret: Option<String> },
    Push { url: String, token: Option<String> },
    Relay(RelayConfig),
}

impl Channel {
    pub fn name(&self) -> &'static str {
        match self {
            Channel::Webhook { .. } => "webhook",
            Channel::Push { .. } => "push",
            Channel::Relay(_) => "relay",
        }
    }

    pub async fn send(
        &self,
        msg: &Message,
        http: &reqwest::Client,
        minimal: bool,
    ) -> Result<(), String> {
        match self {
            Channel::Webhook { url, secret } => webhook(url, secret.as_deref(), msg, http).await,
            Channel::Push { url, token } => push(url, token.as_deref(), msg, http, minimal).await,
            Channel::Relay(cfg) => relay(cfg, msg).await,
        }
    }
}

/// Why the relay may not be used with these recipients: a recipient in a
/// local domain would bring the alert back through this server's own
/// receiver, the path whose failures it reports.
pub fn relay_refusal(cfg: &RelayConfig, local_domains: &[String]) -> Option<String> {
    if cfg.to.is_empty() {
        return Some("MAILRS_OPS_RELAY_TO names no recipient".into());
    }
    let local: Vec<&String> = cfg
        .to
        .iter()
        .filter(|addr| {
            addr.rsplit_once('@')
                .is_some_and(|(_, d)| local_domains.iter().any(|l| l.eq_ignore_ascii_case(d)))
        })
        .collect();
    match local.is_empty() {
        true => None,
        false => Some(format!(
            "recipients in a local domain (MAILRS_LOCAL_DOMAINS) would come back through this server: {local:?}"
        )),
    }
}

async fn check(resp: Result<reqwest::Response, reqwest::Error>) -> Result<(), String> {
    match resp {
        Ok(r) if r.status().is_success() => Ok(()),
        Ok(r) => Err(format!("answered {}", r.status())),
        Err(e) => Err(format!("not delivered: {e}")),
    }
}

async fn webhook(
    url: &str,
    secret: Option<&str>,
    msg: &Message,
    http: &reqwest::Client,
) -> Result<(), String> {
    let body = msg.json().to_string();
    let mut req = http
        .post(url)
        .timeout(TIMEOUT)
        .header(reqwest::header::CONTENT_TYPE, "application/json");
    if let Some(secret) = secret {
        let sig = mailrs_webhook_signature::sign(secret.as_bytes(), body.as_bytes());
        req = req.header(
            "x-mailrs-signature",
            mailrs_webhook_signature::format_header(&sig),
        );
    }
    check(req.body(body).send().await).await
}

async fn push(
    url: &str,
    token: Option<&str>,
    msg: &Message,
    http: &reqwest::Client,
    minimal: bool,
) -> Result<(), String> {
    let (title, body) = msg.push_text(minimal);
    let priority = match msg.critical() {
        true => "5",
        false => "3",
    };
    let mut req = http
        .post(url)
        .timeout(TIMEOUT)
        .header("Title", title)
        .header("Priority", priority);
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    check(req.body(body).send().await).await
}

/// The relay mail: plain text, base64 so the Chinese text needs no 8BITMIME.
pub fn mail_bytes(cfg: &RelayConfig, msg: &Message) -> Vec<u8> {
    use base64::Engine as _;
    let (subject, body) = msg.mail_text();
    let encoded = base64::engine::general_purpose::STANDARD.encode(body.as_bytes());
    let lines: Vec<&str> = encoded
        .as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect();
    let date =
        chrono::DateTime::from_timestamp(msg.at, 0).map_or_else(String::new, |d| d.to_rfc2822());
    let domain = cfg.from.rsplit_once('@').map_or("localhost", |(_, d)| d);
    format!(
        "From: {}\r\nTo: {}\r\nSubject: {}\r\nDate: {date}\r\nMessage-ID: <ops.{}@{domain}>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n",
        cfg.from,
        cfg.to.join(", "),
        subject.replace(['\r', '\n'], " "),
        msg.id,
        lines.join("\r\n"),
    )
    .into_bytes()
}

async fn relay(cfg: &RelayConfig, msg: &Message) -> Result<(), String> {
    use mailrs_smtp_client::SmtpConnection;
    let say = |what: &str, e: std::io::Error| format!("relay {what}: {e}");
    let mut conn = SmtpConnection::connect(&cfg.host, cfg.port)
        .await
        .map_err(|e| say("connect", e))?;
    let helo = cfg
        .from
        .rsplit_once('@')
        .map_or("localhost", |(_, d)| d)
        .to_string();
    conn.ehlo(&helo).await.map_err(|e| say("ehlo", e))?;
    if cfg.starttls {
        conn = conn
            .starttls(&cfg.host)
            .await
            .map_err(|e| say("starttls", e))?;
        conn.ehlo(&helo).await.map_err(|e| say("ehlo", e))?;
    }
    if let (Some(user), Some(password)) = (&cfg.user, &cfg.password) {
        let r = conn
            .auth_plain(user, password)
            .await
            .map_err(|e| say("auth", e))?;
        if !r.is_positive() {
            return Err(format!("relay refused the login: {}", r.code));
        }
    }
    let to: Vec<&str> = cfg.to.iter().map(String::as_str).collect();
    let r = conn
        .deliver(&cfg.from, &to, &mail_bytes(cfg, msg))
        .await
        .map_err(|e| say("deliver", e))?;
    let _ = conn.quit().await;
    match r.is_positive() {
        true => Ok(()),
        false => Err(format!(
            "relay refused the message: {} {}",
            r.code,
            r.message()
        )),
    }
}
