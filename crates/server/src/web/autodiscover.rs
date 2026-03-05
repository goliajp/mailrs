use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use super::WebState;

pub(super) async fn autodiscover_outlook(State(state): State<Arc<WebState>>) -> impl IntoResponse {
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
        [("content-type", "application/xml; charset=utf-8")],
        xml,
    )
}

pub(super) async fn autoconfig_mozilla(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let hostname = &state.hostname;
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
        [("content-type", "application/xml; charset=utf-8")],
        xml,
    )
}
