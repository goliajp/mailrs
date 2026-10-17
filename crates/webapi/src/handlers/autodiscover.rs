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

/// An unauthenticated caller picks this value and it lands inside the
/// profile, so only a plain `local@domain` address is accepted.
fn is_plain_email(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else {
        return false;
    };
    let local_ok = !local.is_empty()
        && local.len() <= 64
        && local
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-/=?^_`{|}~.".contains(c));
    let domain_ok = domain.len() <= 253
        && domain.contains('.')
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
    s.len() <= 254 && local_ok && domain_ok
}

fn render_mobileconfig(email: &str, hostname: &str, uuid: &str) -> String {
    let email = quick_xml::escape::escape(email);
    let hostname = quick_xml::escape::escape(hostname);
    let uuid = quick_xml::escape::escape(uuid);
    format!(
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
    )
}

/// GET /.well-known/apple-mobileconfig — iOS/macOS Mail signed profile
/// stub. Returns an unsigned .mobileconfig; downstream ops can sign it.
/// An `emailaddress` that is not a plain address is refused with 400.
pub async fn apple_mobileconfig(Query(q): Query<MobileConfigQuery>) -> impl IntoResponse {
    let email = match q.emailaddress {
        Some(e) if is_plain_email(&e) => e,
        Some(_) => {
            return (
                StatusCode::BAD_REQUEST,
                "emailaddress is not an email address",
            )
                .into_response();
        }
        None => "%EMAILADDRESS%".into(),
    };
    let xml = render_mobileconfig(&email, &hostname(), &uuid::Uuid::new_v4().to_string());
    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "application/x-apple-aspen-config; charset=utf-8",
        )],
        xml,
    )
        .into_response()
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

#[cfg(test)]
mod tests {
    use super::*;
    use quick_xml::events::Event;

    // a full well-formedness pass (end names are checked by default);
    // returns how many <key> elements the document has
    fn key_count(xml: &str) -> usize {
        let mut reader = quick_xml::Reader::from_str(xml);
        let mut keys = 0;
        loop {
            match reader.read_event().expect("well-formed plist") {
                Event::Start(e) if e.name().into_inner() == "key" => keys += 1,
                Event::Eof => return keys,
                _ => {}
            }
        }
    }

    async fn call(email: Option<&str>) -> (StatusCode, String) {
        let q = MobileConfigQuery {
            emailaddress: email.map(str::to_string),
        };
        let resp = apple_mobileconfig(Query(q)).await.into_response();
        let status = resp.status();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn markup_in_emailaddress_is_refused() {
        for bad in [
            "a<b@golia.jp",
            "a\"b@golia.jp",
            "x@golia.jp</string><key>PayloadType</key><string>evil",
            "x@golia.jp&amp;",
            "no-at-sign",
            "a@b@golia.jp",
            "a@nodot",
            "",
        ] {
            let (status, _) = call(Some(bad)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{bad:?}");
        }
    }

    #[tokio::test]
    async fn plain_address_is_embedded() {
        let (status, body) = call(Some("lihao@golia.jp")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(key_count(&body), 23);
        assert_eq!(body.matches("<string>lihao@golia.jp</string>").count(), 4);
    }

    #[tokio::test]
    async fn absent_address_keeps_the_placeholder() {
        let (status, body) = call(None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("<string>%EMAILADDRESS%</string>"));
    }

    #[test]
    fn every_interpolated_value_is_escaped() {
        // `&` and `'` are legal in a local part, so they pass validation
        // and must be escaped; the hostname comes from the environment
        let email = "o'neil&co@golia.jp";
        assert!(is_plain_email(email));
        let hostname = "mail.golia.jp\"</string><key>x</key><string>";
        let xml = render_mobileconfig(email, hostname, "u<1>");
        // an unescaped hostname would add a 24th <key>
        assert_eq!(key_count(&xml), 23);
        assert_eq!(
            xml.matches("<string>o&apos;neil&amp;co@golia.jp</string>")
                .count(),
            4
        );
        assert!(xml.contains(
            "<string>mail.golia.jp&quot;&lt;/string&gt;&lt;key&gt;x&lt;/key&gt;&lt;string&gt;</string>"
        ));
        assert!(xml.contains("<string>jp.golia.mailrs.u&lt;1&gt;</string>"));
    }
}
