use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use super::WebState;

/// What a mail app is told when no IMAP listener is on.
fn not_served() -> axum::response::Response {
    (StatusCode::NOT_FOUND, "IMAP is not served").into_response()
}

pub(super) async fn autodiscover_outlook(
    State(state): State<Arc<WebState>>,
) -> axum::response::Response {
    // the port the core's listener binds, read from the variables it reads
    let Some(imap) = mailrs_mail_protocols::listen::imap_for_clients() else {
        return not_served();
    };
    let (port, tls) = match imap.implicit_tls {
        true => (imap.port, "<SSL>on</SSL>"),
        false => (
            imap.port,
            "<SSL>off</SSL>\n        <Encryption>TLS</Encryption>",
        ),
    };
    let hostname = &state.hostname;
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
        <Port>{port}</Port>
        {tls}
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
        [("content-type", "application/xml; charset=utf-8")],
        xml,
    )
        .into_response()
}

pub(super) async fn autoconfig_mozilla(
    State(state): State<Arc<WebState>>,
) -> axum::response::Response {
    // the port the core's listener binds, read from the variables it reads
    let Some(imap) = mailrs_mail_protocols::listen::imap_for_clients() else {
        return not_served();
    };
    let (port, socket) = match imap.implicit_tls {
        true => (imap.port, "SSL"),
        false => (imap.port, "STARTTLS"),
    };
    let hostname = &state.hostname;
    let xml = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<clientConfig version="1.1">
  <emailProvider id="{hostname}">
    <domain>%EMAILDOMAIN%</domain>
    <incomingServer type="imap">
      <hostname>{hostname}</hostname>
      <port>{port}</port>
      <socketType>{socket}</socketType>
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
        [("content-type", "application/xml; charset=utf-8")],
        xml,
    )
        .into_response()
}
