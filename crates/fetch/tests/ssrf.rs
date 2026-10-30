//! The fetcher against a server it is allowed to reach, standing in for
//! a sender's web server: it brings back a clean image, and refuses every
//! way a URL in a message could steer it inside — a redirect into a
//! private range, a name that resolves to one, the bridge gateway, too
//! many hops, something that is not an image, something too big.
//!
//! Loopback is refused in production; here the one test server's address
//! and port are let through and every other private address is not.

use std::io::Cursor;
use std::net::{IpAddr, Ipv4Addr};

use axum::Router;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::get;
use mailrs_fetch::{Failed, Limits, Policy, Refused};

/// A JPEG with an EXIF block naming where it was taken.
fn photo_with_location() -> Vec<u8> {
    let img = image::RgbImage::from_pixel(8, 8, image::Rgb([200, 30, 30]));
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode_image(&img)
        .unwrap();
    // an APP1 Exif segment right after SOI, carrying a marker to look for
    let payload = b"Exif\0\0GPS-35.6895N-139.6917E";
    let mut app1 = vec![0xFF, 0xE1];
    app1.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    app1.extend_from_slice(payload);
    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&app1);
    out.extend_from_slice(&jpeg[2..]);
    out
}

async fn serve() -> u16 {
    let photo = photo_with_location();
    let app = Router::new()
        .route(
            "/photo.jpg",
            get(move || {
                let photo = photo.clone();
                async move { ([(header::CONTENT_TYPE, "image/jpeg")], photo) }
            }),
        )
        .route(
            "/to-inside",
            get(|| async { redirect("http://10.0.0.1/a.png") }),
        )
        .route(
            "/to-metadata",
            get(|| async { redirect("http://169.254.169.254/latest/") }),
        )
        .route("/hop/1", get(|| async { redirect("/hop/2") }))
        .route("/hop/2", get(|| async { redirect("/hop/3") }))
        .route("/hop/3", get(|| async { redirect("/hop/4") }))
        .route("/hop/4", get(|| async { redirect("/photo.jpg") }))
        .route(
            "/page",
            get(|| async { ([(header::CONTENT_TYPE, "text/html")], "<html>") }),
        )
        .route(
            "/huge",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/png")],
                    vec![0u8; 6 * 1024 * 1024],
                )
            }),
        )
        .route(
            "/broken",
            get(|| async { ([(header::CONTENT_TYPE, "image/png")], b"not a png".to_vec()) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    port
}

fn redirect(to: &'static str) -> impl IntoResponse {
    (StatusCode::FOUND, [(header::LOCATION, to)])
}

fn test_server_or_public(ip: IpAddr) -> Result<(), Refused> {
    match ip == IpAddr::V4(Ipv4Addr::LOCALHOST) {
        true => Ok(()),
        false => mailrs_fetch::address::public(ip),
    }
}

/// `sender.test` resolves to the test server; `rebind.test` to it and to a
/// private address; `bridge.test` to a docker bridge gateway.
fn lookup(
    host: &str,
    _port: u16,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<Vec<IpAddr>>> + Send>> {
    let ips: Vec<IpAddr> = match host {
        "sender.test" => vec!["127.0.0.1".parse().unwrap()],
        "rebind.test" => vec!["127.0.0.1".parse().unwrap(), "10.1.2.3".parse().unwrap()],
        "bridge.test" => vec!["172.18.0.1".parse().unwrap()],
        _ => vec![],
    };
    Box::pin(async move { Ok(ips) })
}

#[tokio::test]
async fn only_a_clean_image_from_outside_comes_back() {
    let port = serve().await;
    let ports = [port];
    let policy = Policy {
        lookup: &lookup,
        allowed: &test_server_or_public,
        ports: &ports,
    };
    let url = |host: &str, path: &str| format!("http://{host}:{port}{path}");
    let fetch = |u: String| {
        let policy = &policy;
        async move { mailrs_fetch::fetch_with(&u, Limits::DEFAULT, policy).await }
    };

    let image = fetch(url("sender.test", "/photo.jpg"))
        .await
        .expect("the photo");
    assert_eq!(image.content_type, "image/jpeg");
    assert!(
        !image.bytes.windows(4).any(|w| w == b"Exif"),
        "EXIF survived"
    );
    assert!(
        !image.bytes.windows(3).any(|w| w == b"GPS"),
        "the location survived"
    );
    assert!(image::load_from_memory(&image.bytes).is_ok());

    let refused = |r: Result<_, Failed>| matches!(r, Err(Failed::Refused(_)));
    assert!(
        refused(fetch(url("sender.test", "/to-inside")).await),
        "a redirect inside"
    );
    assert!(
        refused(fetch(url("sender.test", "/to-metadata")).await),
        "a redirect to metadata"
    );
    assert!(
        refused(fetch(url("rebind.test", "/photo.jpg")).await),
        "one private answer of two"
    );
    assert!(
        refused(fetch(url("bridge.test", "/photo.jpg")).await),
        "the bridge gateway"
    );
    assert!(matches!(
        fetch(url("sender.test", "/hop/1")).await,
        Err(Failed::TooManyRedirects)
    ));
    assert!(matches!(
        fetch(url("sender.test", "/page")).await,
        Err(Failed::NotAnImage(_))
    ));
    assert!(matches!(
        fetch(url("sender.test", "/huge")).await,
        Err(Failed::TooLarge)
    ));
    assert!(matches!(
        fetch(url("sender.test", "/broken")).await,
        Err(Failed::Unreadable(_))
    ));
    // the test server's own address, written as a number, is still refused
    // on a port the policy does not allow
    assert!(refused(
        fetch(format!("http://2130706433:{}/photo.jpg", port + 1)).await
    ));
}

#[tokio::test]
async fn an_image_wider_than_allowed_is_refused_before_it_is_decoded() {
    // a PNG header that claims 100000 pixels a side; its body is never read
    let img = image::RgbImage::new(1, 1);
    let mut png = Vec::new();
    img.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    // IHDR width and height live at bytes 16..24
    png[16..20].copy_from_slice(&100_000u32.to_be_bytes());
    png[20..24].copy_from_slice(&100_000u32.to_be_bytes());
    let refused = mailrs_fetch::reencode(&png, 4096);
    assert!(refused.is_err(), "decoded an image of 100000 pixels a side");
}
