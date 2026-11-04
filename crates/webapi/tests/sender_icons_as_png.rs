//! The mobile clients decode neither SVG nor ICO, and BIMI logos are SVG,
//! so they ask `/api/icon/{domain}?format=png` and get a PNG of at most
//! 128 px a side. The web keeps the stored bytes as they are.
//!
//! The icons are seeded into the shared cache, so nothing here leaves the
//! machine.

mod live;

use std::io::Cursor;

const KEVY: &str = "mem://webapi-sender-icons-as-png";
const USER: &str = "alice@example.test";

/// The shape of a BIMI tiny-ps logo: a rounded square with a mark on it.
const BIMI_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" version="1.2" baseProfile="tiny-ps" viewBox="0 0 512 512"><title>Example</title><rect width="512" height="512" rx="96" fill="#1a73e8"/><path d="M128 256 L256 128 L384 256 L256 384 Z" fill="#ffffff"/></svg>"##;

const WIDE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 100"><rect width="400" height="100" fill="#ff0000"/></svg>"##;

fn ico_32() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(32, 32, image::Rgba([0, 128, 0, 255]));
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Ico)
        .expect("encode ico");
    out
}

fn seed(c: &mut kevy_client::Connection, domain: &str, ct: &str, body: &[u8]) {
    let key = format!("webapi:icon:v1:{domain}");
    c.hset(key.as_bytes(), &[(b"ct", ct.as_bytes()), (b"body", body)])
        .expect("seed icon");
}

async fn get(live: &live::Live, bearer: &str, path: &str) -> (u16, String, Vec<u8>) {
    let res = live
        .http
        .get(format!("{}{path}", live.base))
        .bearer_auth(bearer)
        .send()
        .await
        .expect("request");
    let status = res.status().as_u16();
    let ct = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    (status, ct, res.bytes().await.expect("body").to_vec())
}

fn decode_png(bytes: &[u8]) -> image::RgbaImage {
    assert!(
        bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "not a PNG: {:?}",
        &bytes[..bytes.len().min(16)]
    );
    image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .expect("decode png")
        .into_rgba8()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mobile_clients_get_a_png_and_the_web_keeps_its_bytes() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(USER, "pw").await;
    let token = live.login(USER, "pw").await;
    let mut c = kevy_client::Connection::connect(KEVY).expect("kevy");

    let ico = ico_32();
    seed(&mut c, "bimi.example", "image/svg+xml", BIMI_SVG.as_bytes());
    seed(&mut c, "wide.example", "image/svg+xml", WIDE_SVG.as_bytes());
    seed(&mut c, "ico.example", "image/x-icon", &ico);
    c.set(b"webapi:icon:v1:miss:none.example", b"1")
        .expect("seed miss");

    // the web: unchanged, byte for byte
    let (status, ct, body) = get(&live, &token, "/api/icon/bimi.example").await;
    assert_eq!((status, ct.as_str()), (200, "image/svg+xml"));
    assert_eq!(body, BIMI_SVG.as_bytes());
    let (status, ct, body) = get(&live, &token, "/api/icon/ico.example").await;
    assert_eq!((status, ct.as_str()), (200, "image/x-icon"));
    assert_eq!(body, ico);

    // a BIMI logo, drawn: square, corners still transparent, mark on top
    let (status, ct, body) = get(&live, &token, "/api/icon/bimi.example?format=png").await;
    assert_eq!((status, ct.as_str()), (200, "image/png"));
    let png = decode_png(&body);
    assert_eq!(png.dimensions(), (128, 128));
    assert_eq!(png.get_pixel(0, 0)[3], 0, "the rounded corner was filled");
    assert_eq!(png.get_pixel(64, 64).0, [255, 255, 255, 255], "the mark");
    let side = png.get_pixel(20, 64).0;
    assert_eq!((side[2], side[3]), (0xe8, 255), "the square: {side:?}");

    // the drawing is kept, next to the original
    let cached = c
        .get(b"webapi:icon:v1:png:bimi.example")
        .expect("read cache")
        .expect("the PNG was not cached");
    assert_eq!(cached, body);

    // a wide logo keeps its shape
    let (status, _, body) = get(&live, &token, "/api/icon/wide.example?format=png").await;
    assert_eq!(status, 200);
    assert_eq!(decode_png(&body).dimensions(), (128, 32));

    // an .ico becomes a PNG of the same size
    let (status, ct, body) = get(&live, &token, "/api/icon/ico.example?format=png").await;
    assert_eq!((status, ct.as_str()), (200, "image/png"));
    let png = decode_png(&body);
    assert_eq!(png.dimensions(), (32, 32));
    assert_eq!(png.get_pixel(5, 5).0, [0, 128, 0, 255]);

    // no icon is still 204, in either form
    let (status, _, body) = get(&live, &token, "/api/icon/none.example?format=png").await;
    assert_eq!((status, body.len()), (204, 0));
    let (status, _, _) = get(&live, &token, "/api/icon/none.example").await;
    assert_eq!(status, 204);

    // a format there is no converter for is the caller's mistake
    let (status, _, _) = get(&live, &token, "/api/icon/bimi.example?format=gif").await;
    assert_eq!(status, 400);
}
