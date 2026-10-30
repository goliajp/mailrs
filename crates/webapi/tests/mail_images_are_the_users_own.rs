//! A prefetched image is served to the user whose mail it came in, and to
//! nobody else asking by the same name. There is no image proxy that
//! fetches a URL on request: webapi itself answers 404 to the path the old
//! one used, with nothing in front of it.

mod live;

use reqwest::Method;

const KEVY: &str = "mem://webapi-mail-images-are-the-users-own";
const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_image_is_its_recipients_and_no_url_is_fetched_on_request() {
    let cache = tempfile::tempdir().unwrap();
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_IMAGE_CACHE_DIR", cache.path());
    }
    let live = live::start(KEVY).await;
    for who in [ALICE, BOB] {
        live.add_account(who, "pw").await;
    }
    let url = "https://tracker.example/open.gif?u=alice";
    let path = mailrs_core_sidestate::image_cache::path(cache.path(), ALICE, url);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"\x89PNG\r\n\x1a\nfake").unwrap();
    let name = mailrs_core_sidestate::image_cache::name(url);

    let get = |token: String, path: String| {
        let live = &live;
        async move {
            live.http
                .get(format!("{}{path}", live.base))
                .bearer_auth(token)
                .send()
                .await
                .unwrap()
        }
    };
    let alice = live.login(ALICE, "pw").await;
    let bob = live.login(BOB, "pw").await;
    let res = get(alice.clone(), format!("/api/mail/image/{name}")).await;
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["content-type"], "image/png");
    assert_eq!(res.headers()["x-content-type-options"], "nosniff");
    assert_eq!(
        res.bytes().await.unwrap().as_ref(),
        b"\x89PNG\r\n\x1a\nfake"
    );

    assert_eq!(
        get(bob.clone(), format!("/api/mail/image/{name}"))
            .await
            .status(),
        404
    );
    assert_eq!(
        get(alice.clone(), "/api/mail/image/..%2F..%2Fetc".into())
            .await
            .status(),
        404
    );
    let (status, _) = live
        .call(
            Method::GET,
            "/api/proxy/image?url=http%3A%2F%2F169.254.169.254%2F",
            &alice,
            None,
        )
        .await;
    assert_eq!(status, 404, "an image proxy answers here");
}
