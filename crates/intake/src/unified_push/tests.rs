use std::net::SocketAddr;

use aws_lc_rs::agreement::{ECDH_P256, PrivateKey};
use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair};
use base64::Engine as _;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use super::*;
use mailrs_core_sidestate::families::notify;

/// One request a distributor received.
struct Received {
    path: String,
    headers: String,
    body: Vec<u8>,
}

/// A distributor's push server on loopback: answers 410 for paths with
/// `gone` in them and 201 for the rest, and hands over what it was sent.
async fn distributor() -> (SocketAddr, mpsc::UnboundedReceiver<Received>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let mut raw = Vec::new();
            let mut buf = [0u8; 4096];
            let head_end = loop {
                let n = sock.read(&mut buf).await.unwrap();
                raw.extend_from_slice(&buf[..n]);
                if let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i + 4;
                }
            };
            let headers = String::from_utf8_lossy(&raw[..head_end]).to_ascii_lowercase();
            let length: usize = headers
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .map_or(0, |v| v.trim().parse().unwrap());
            while raw.len() < head_end + length {
                let n = sock.read(&mut buf).await.unwrap();
                raw.extend_from_slice(&buf[..n]);
            }
            let path = headers.split(' ').nth(1).unwrap_or_default().to_string();
            let status = match path.contains("gone") {
                true => "410 Gone",
                false => "201 Created",
            };
            let reply =
                format!("HTTP/1.1 {status}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
            sock.write_all(reply.as_bytes()).await.unwrap();
            let _ = tx.send(Received {
                path,
                headers,
                body: raw[head_end..head_end + length].to_vec(),
            });
        }
    });
    (addr, rx)
}

fn vapid() -> Vapid {
    let doc = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
        .unwrap();
    let b64 = base64::engine::general_purpose::STANDARD.encode(doc.as_ref());
    let pem = format!("-----BEGIN PRIVATE KEY-----\n{b64}\n-----END PRIVATE KEY-----\n");
    Vapid::from_pem(&pem, "mailto:postmaster@mail.example.test").unwrap()
}

/// A phone: its private key, and the device it registers with it.
fn phone(endpoint: &str, created_at: i64) -> (PrivateKey, Device) {
    let key = PrivateKey::generate(&ECDH_P256).unwrap();
    let device = Device {
        endpoint: endpoint.to_string(),
        p256dh: mailrs_webpush::b64url(key.compute_public_key().unwrap().as_ref()),
        auth: mailrs_webpush::b64url(&[7u8; 16]),
        name: "Pixel".into(),
        created_at,
    };
    (key, device)
}

#[tokio::test]
async fn new_mail_pings_each_phone_with_nothing_of_the_message_and_a_gone_one_is_dropped() {
    let (addr, mut received) = distributor().await;
    let vapid = vapid();
    let now = 1_793_500_000;
    let mut conn = kevy_client::Connection::connect("mem://unified-push-round").unwrap();
    let (live_key, live) = phone(&format!("http://{addr}/upLIVE"), 1);
    let (_, gone) = phone(&format!("http://{addr}/upgone"), 2);
    unified_push::add(&mut conn, "me@x.test", &live).unwrap();
    unified_push::add(&mut conn, "me@x.test", &gone).unwrap();

    let devices = unified_push::list(&mut conn, "me@x.test").unwrap();
    let outcomes = round(devices, |sub| {
        let vapid = &vapid;
        async move { send::post(vapid, &sub, PING, now, "127.0.0.1", addr).await }
    })
    .await;
    assert_eq!(
        outcomes.iter().map(|(_, o)| o.clone()).collect::<Vec<_>>(),
        vec![Outcome::Sent, Outcome::Gone]
    );
    settle(&mut conn, "me@x.test", &outcomes, now).unwrap();

    let first = received.recv().await.unwrap();
    assert_eq!(first.path, "/uplive");
    assert!(first.headers.contains("content-encoding: aes128gcm"));
    assert!(first.headers.contains("authorization: vapid t="));
    let plain = mailrs_webpush::ece::decrypt(&first.body, &live_key, &[7u8; 16]).unwrap();
    assert_eq!(
        plain, PING,
        "the phone reads the fixed ping and nothing else"
    );
    assert!(
        !String::from_utf8_lossy(&plain).contains('@'),
        "no address in the ping"
    );
    assert_eq!(received.recv().await.unwrap().path, "/upgone");

    let left = unified_push::list(&mut conn, "me@x.test").unwrap();
    assert_eq!(left.len(), 1, "the 410 dropped its device");
    assert_eq!(left[0].1.endpoint, live.endpoint);
    let stats = notify::stats(&mut conn, "me@x.test", 1, now).unwrap();
    assert_eq!(stats[0].counts[CHANNEL]["sent"], 1);
    assert_eq!(stats[0].counts[CHANNEL]["failed"], 1);
    let used = conn
        .hgetall(mailrs_core_sidestate::keys::push::used_key("me@x.test").as_bytes())
        .unwrap();
    let live_field = format!("{CHANNEL}:{}", unified_push::id_of(&live.endpoint));
    assert_eq!(
        used,
        vec![live_field.into_bytes(), now.to_string().into_bytes()],
        "only the push that got through is recorded as a use"
    );
}

#[tokio::test]
async fn the_real_sender_refuses_a_loopback_distributor_without_connecting() {
    let (addr, mut received) = distributor().await;
    let vapid = vapid();
    for endpoint in [
        format!("http://{addr}/upLIVE"),
        format!("https://127.0.0.1:{}/upLIVE", addr.port()),
        "https://localhost/upLIVE".to_string(),
    ] {
        let (_, device) = phone(&endpoint, 1);
        let outcome =
            send::send_to_distributor(&vapid, &subscription(&device).unwrap(), PING, 1).await;
        assert!(
            matches!(outcome, Outcome::Failed(_)),
            "{endpoint}: {outcome:?}"
        );
    }
    assert!(received.try_recv().is_err(), "nothing reached the receiver");
}

#[tokio::test]
async fn a_device_whose_keys_do_not_decode_is_gone_without_a_request() {
    let (_, mut device) = phone("https://ntfy.example.org/upX", 1);
    device.p256dh = "not base64 !!".into();
    let sent = std::cell::Cell::new(0);
    let outcomes = round(vec![("id".into(), device)], |_| {
        sent.set(sent.get() + 1);
        async { Outcome::Sent }
    })
    .await;
    assert_eq!(outcomes, vec![("id".to_string(), Outcome::Gone)]);
    assert_eq!(sent.get(), 0, "nothing is sent to a device without keys");
}
