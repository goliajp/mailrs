//! What mail apps are told about IMAP is the port the IMAP listener binds,
//! read from the same variables the listener reads.
//!
//! An integration test because it sets those variables, and only a
//! process of its own may.

use axum::extract::Query;
use axum::response::IntoResponse;
use mailrs_webapi::handlers::autodiscover;

async fn body(resp: axum::response::Response) -> (u16, String) {
    let status = resp.status().as_u16();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

async fn all_three() -> Vec<(u16, String)> {
    let query = autodiscover::MobileConfigQuery { emailaddress: None };
    vec![
        body(autodiscover::autoconfig_mozilla().await.into_response()).await,
        body(autodiscover::autodiscover_outlook().await.into_response()).await,
        body(
            autodiscover::apple_mobileconfig(Query(query))
                .await
                .into_response(),
        )
        .await,
    ]
}

#[tokio::test]
async fn every_autoconfig_names_the_port_the_listener_binds() {
    // SAFETY: the only test in this binary; set before anything reads them
    unsafe { std::env::set_var("MAILRS_IMAPS_BIND", "0.0.0.0:1993") };
    let [thunderbird, outlook, apple] = <[_; 3]>::try_from(all_three().await).unwrap();
    assert!(
        thunderbird.1.contains("<port>1993</port>"),
        "{}",
        thunderbird.1
    );
    assert!(thunderbird.1.contains("<socketType>SSL</socketType>"));
    assert!(outlook.1.contains("<Port>1993</Port>"), "{}", outlook.1);
    assert!(
        apple
            .1
            .contains("<key>IncomingMailServerPortNumber</key><integer>1993</integer>"),
        "{}",
        apple.1
    );

    // no implicit TLS: the plaintext port, upgraded with STARTTLS
    unsafe { std::env::set_var("MAILRS_IMAPS_BIND", "off") };
    unsafe { std::env::set_var("MAILRS_IMAP_BIND", "0.0.0.0:1143") };
    let [thunderbird, outlook, apple] = <[_; 3]>::try_from(all_three().await).unwrap();
    assert!(
        thunderbird.1.contains("<port>1143</port>"),
        "{}",
        thunderbird.1
    );
    assert!(thunderbird.1.contains("<socketType>STARTTLS</socketType>"));
    assert!(outlook.1.contains("<Port>1143</Port>"), "{}", outlook.1);
    assert!(
        outlook.1.contains("<Encryption>TLS</Encryption>"),
        "{}",
        outlook.1
    );
    assert!(apple.1.contains("<integer>1143</integer>"), "{}", apple.1);

    // no IMAP at all: nothing to announce
    unsafe { std::env::set_var("MAILRS_IMAP_BIND", "off") };
    for (status, text) in all_three().await {
        assert_eq!(status, 404, "{text}");
    }
}
