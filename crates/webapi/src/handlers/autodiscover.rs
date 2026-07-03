//! Autodiscover / autoconfig / MTA-STS endpoints — port of the
//! monolith handlers at `crates/server/src/web/autodiscover.rs` and
//! `admin/policy.rs`. Storage-free except for MTA-STS which reads
//! its config from `MAILRS_MTA_STS_*` env vars (same shape as the
//! monolith's `WebState::with_mta_sts`).

use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;

fn hostname() -> String {
    std::env::var("MAILRS_HOSTNAME").unwrap_or_else(|_| "mail.golia.jp".into())
}

/// GET /autodiscover/autodiscover.xml + POST alias — Outlook / iOS Mail.
pub async fn autodiscover_outlook() -> impl IntoResponse {
    let hostname = hostname();
    let xml = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<Autodiscover xmlns="http://schemas.microsoft.com/exchange/autodiscover/responseschema/2006">
  <Response xmlns="http://schemas.microsoft.com/exchange/autodiscover/outlook/responseschema/2006a">
    <Account>
      <AccountType>email</AccountType>
      <Action>settings</Action>
      <Protocol>
        <Type>IMAP</Type>
        <Server>{hostname}</Server>
        <Port>993</Port>
        <SSL>on</SSL>
        <SPA>off</SPA>
        <LoginName>%EMAILADDRESS%</LoginName>
      </Protocol>
      <Protocol>
        <Type>SMTP</Type>
        <Server>{hostname}</Server>
        <Port>465</Port>
        <SSL>on</SSL>
        <SPA>off</SPA>
        <LoginName>%EMAILADDRESS%</LoginName>
      </Protocol>
    </Account>
  </Response>
</Autodiscover>"#
    );
    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "application/xml; charset=utf-8",
        )],
        xml,
    )
}

/// GET /.well-known/autoconfig/mail/config-v1.1.xml — Thunderbird.
pub async fn autoconfig_mozilla() -> impl IntoResponse {
    let hostname = hostname();
    let xml = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<clientConfig version="1.1">
  <emailProvider id="{hostname}">
    <domain>%EMAILDOMAIN%</domain>
    <incomingServer type="imap">
      <hostname>{hostname}</hostname>
      <port>993</port>
      <socketType>SSL</socketType>
      <authentication>password-cleartext</authentication>
      <username>%EMAILADDRESS%</username>
    </incomingServer>
    <outgoingServer type="smtp">
      <hostname>{hostname}</hostname>
      <port>465</port>
      <socketType>SSL</socketType>
      <authentication>password-cleartext</authentication>
      <username>%EMAILADDRESS%</username>
    </outgoingServer>
  </emailProvider>
</clientConfig>"#
    );
    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "application/xml; charset=utf-8",
        )],
        xml,
    )
}

#[derive(Deserialize)]
pub struct MobileConfigQuery {
    /// Optional email address to pre-fill; passed through as
    /// `EmailAddress` in the profile.
    #[serde(default)]
    pub emailaddress: Option<String>,
}

/// GET /.well-known/apple-mobileconfig — iOS/macOS Mail signed profile
/// stub. Returns an unsigned .mobileconfig; downstream ops can sign it.
pub async fn apple_mobileconfig(Query(q): Query<MobileConfigQuery>) -> impl IntoResponse {
    let hostname = hostname();
    let email = q.emailaddress.unwrap_or_else(|| "%EMAILADDRESS%".into());
    let uuid = uuid::Uuid::new_v4();
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>PayloadContent</key>
  <array>
    <dict>
      <key>EmailAccountName</key><string>{email}</string>
      <key>EmailAccountType</key><string>EmailTypeIMAP</string>
      <key>EmailAddress</key><string>{email}</string>
      <key>IncomingMailServerAuthentication</key><string>EmailAuthPassword</string>
      <key>IncomingMailServerHostName</key><string>{hostname}</string>
      <key>IncomingMailServerPortNumber</key><integer>993</integer>
      <key>IncomingMailServerUseSSL</key><true/>
      <key>IncomingMailServerUsername</key><string>{email}</string>
      <key>OutgoingMailServerAuthentication</key><string>EmailAuthPassword</string>
      <key>OutgoingMailServerHostName</key><string>{hostname}</string>
      <key>OutgoingMailServerPortNumber</key><integer>465</integer>
      <key>OutgoingMailServerUseSSL</key><true/>
      <key>OutgoingMailServerUsername</key><string>{email}</string>
      <key>PayloadIdentifier</key><string>jp.golia.mailrs.{uuid}</string>
      <key>PayloadType</key><string>com.apple.mail.managed</string>
      <key>PayloadUUID</key><string>{uuid}</string>
      <key>PayloadVersion</key><integer>1</integer>
    </dict>
  </array>
  <key>PayloadDisplayName</key><string>mailrs</string>
  <key>PayloadIdentifier</key><string>jp.golia.mailrs</string>
  <key>PayloadType</key><string>Configuration</string>
  <key>PayloadUUID</key><string>{uuid}</string>
  <key>PayloadVersion</key><integer>1</integer>
</dict>
</plist>"#
    );
    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "application/x-apple-aspen-config; charset=utf-8",
        )],
        xml,
    )
}

/// GET /.well-known/mta-sts.txt — MTA-STS policy in text form. Config
/// comes from env vars mirroring the monolith fields:
///   MAILRS_MTA_STS_MODE      "enforce" / "testing" / "none"
///   MAILRS_MTA_STS_MX        comma-separated hostnames
///   MAILRS_MTA_STS_MAX_AGE   seconds
///   MAILRS_MTA_STS_ID        policy id
pub async fn mta_sts_policy() -> impl IntoResponse {
    let Ok(mode) = std::env::var("MAILRS_MTA_STS_MODE") else {
        return (StatusCode::NOT_FOUND, "MTA-STS not configured".to_string()).into_response();
    };
    let mx = std::env::var("MAILRS_MTA_STS_MX").unwrap_or_default();
    let max_age: u64 = std::env::var("MAILRS_MTA_STS_MAX_AGE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(604800);
    let id = std::env::var("MAILRS_MTA_STS_ID").unwrap_or_else(|_| "mailrs-v1".into());
    let mx_lines: Vec<String> = mx
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|h| format!("mx: {}", h.trim()))
        .collect();
    let body = format!(
        "version: STSv1\nmode: {mode}\n{}\nmax_age: {max_age}\nid: {id}\n",
        mx_lines.join("\n"),
    );
    (StatusCode::OK, body).into_response()
}
