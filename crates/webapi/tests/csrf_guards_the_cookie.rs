//! A request the session cookie authenticates and that changes something
//! must carry the session's CSRF token and, when it names an origin, come
//! from ours. A request authenticated by an `Authorization` header is exempt,
//! and a bad header never falls back to the cookie.

mod live;

use reqwest::Method;

const KEVY: &str = "mem://webapi-csrf-guards-the-cookie";
const USER: &str = "csrf@example.test";
const ORIGIN: &str = "https://mail.example.test";
const WRITE: &str = "/api/spam/whitelist";

fn body() -> serde_json::Value {
    serde_json::json!({ "address": "friend@example.org" })
}

struct Call<'a> {
    live: &'a live::Live,
    cookie: Option<String>,
    headers: Vec<(&'static str, String)>,
}

impl<'a> Call<'a> {
    fn new(live: &'a live::Live) -> Self {
        Call {
            live,
            cookie: None,
            headers: Vec::new(),
        }
    }
    fn cookie(mut self, v: String) -> Self {
        self.cookie = Some(v);
        self
    }
    fn header(mut self, k: &'static str, v: impl Into<String>) -> Self {
        self.headers.push((k, v.into()));
        self
    }
    /// Status, the `error` code of the body, and the `Set-Cookie` values.
    async fn send(self, method: Method, path: &str) -> (u16, String, Vec<String>) {
        let mut req = self
            .live
            .http
            .request(method, format!("{}{path}", self.live.base))
            .json(&body());
        if let Some(c) = self.cookie {
            req = req.header("cookie", c);
        }
        for (k, v) in self.headers {
            req = req.header(k, v);
        }
        let res = req.send().await.expect("request");
        let status = res.status().as_u16();
        let cookies = res
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok().map(String::from))
            .collect();
        let v: serde_json::Value = res.json().await.unwrap_or(serde_json::Value::Null);
        (
            status,
            v["error"].as_str().unwrap_or("").to_string(),
            cookies,
        )
    }
}

fn csrf_from(cookies: &[String]) -> String {
    cookies
        .iter()
        .find_map(|c| c.strip_prefix("mailrs_csrf="))
        .and_then(|rest| rest.split(';').next())
        .expect("a csrf cookie")
        .to_string()
}

#[tokio::test]
async fn cookie_writes_need_the_token_and_our_origin() {
    // SAFETY: the only test in this binary, before anything reads the env
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_WEB_ORIGINS", ORIGIN);
    }
    let live = live::start(KEVY).await;
    live.add_account(USER, "pw").await;
    let token = live.login(USER, "pw").await;
    let session = format!("mailrs_session={token}");

    // no token: refused, and told why; the token cookie comes with the refusal
    let (status, code, cookies) = Call::new(&live)
        .cookie(session.clone())
        .send(Method::POST, WRITE)
        .await;
    assert_eq!((status, code.as_str()), (403, "csrf_missing"));
    let csrf = csrf_from(&cookies);

    // the wrong token
    let (status, code, _) = Call::new(&live)
        .cookie(session.clone())
        .header("x-csrf-token", "0".repeat(64))
        .send(Method::POST, WRITE)
        .await;
    assert_eq!((status, code.as_str()), (403, "csrf_mismatch"));

    // the right token from another site, or from an opaque origin
    for origin in ["https://evil.example", "null"] {
        let (status, code, _) = Call::new(&live)
            .cookie(session.clone())
            .header("x-csrf-token", csrf.clone())
            .header("origin", origin)
            .send(Method::POST, WRITE)
            .await;
        assert_eq!(
            (status, code.as_str()),
            (403, "origin_mismatch"),
            "{origin}"
        );
    }

    // the right token from our origin, and with no origin named
    for origin in [Some(ORIGIN), None] {
        let mut call = Call::new(&live)
            .cookie(format!("{session}; mailrs_csrf={csrf}"))
            .header("x-csrf-token", csrf.clone());
        if let Some(o) = origin {
            call = call.header("origin", o);
        }
        let (status, _, _) = call.send(Method::POST, WRITE).await;
        assert!((200..300).contains(&status), "{origin:?}: {status}");
    }

    // a read needs nothing
    let (status, _, _) = Call::new(&live)
        .cookie(session.clone())
        .send(Method::GET, "/api/auth/me")
        .await;
    assert_eq!(status, 200);

    // a bearer request is exempt
    let (status, _) = live.call(Method::POST, WRITE, &token, Some(body())).await;
    assert!((200..300).contains(&status), "bearer: {status}");

    // a bad Authorization header is a 401, never the cookie's authority
    let (status, _, _) = Call::new(&live)
        .cookie(session.clone())
        .header("authorization", "Bearer junk")
        .send(Method::POST, WRITE)
        .await;
    assert_eq!(status, 401);
    let (status, _, _) = Call::new(&live)
        .cookie(session.clone())
        .header("authorization", "Basic Zm9vOmJhcg==")
        .send(Method::POST, WRITE)
        .await;
    assert_eq!(status, 401);

    // signing in from another site is refused; from ours it works
    for (origin, want) in [("https://evil.example", 403), (ORIGIN, 200)] {
        let res = live
            .http
            .post(format!("{}/api/auth/login", live.base))
            .header("origin", origin)
            .json(&serde_json::json!({ "address": USER, "password": "pw" }))
            .send()
            .await
            .expect("login");
        assert_eq!(res.status().as_u16(), want, "{origin}");
    }

    // a sign-in callback this browser did not start is a failure, whatever
    // the code and state say
    let no_redirects = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client");
    let res = no_redirects
        .get(format!(
            "{}/api/auth/oidc/callback?code=attackers-code&state=attackers-state",
            live.base
        ))
        .header("cookie", session.clone())
        .send()
        .await
        .expect("callback");
    assert!(res.status().is_redirection());
    assert_eq!(res.headers()["location"], "/login?external=failed");
}
