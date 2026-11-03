//! The JMAP methods over a WebSocket: the transport, the same refusals
//! as over HTTP, and catching up after a reconnect.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::{
    Error as WsError, Message, client::IntoClientRequest, http::HeaderValue,
};

use super::{ALICE, BOB, ORIGIN, USING, import, live, message};

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// An upgrade to `/jmap/ws/`: the socket, or the status it was refused with.
async fn upgrade(
    live: &live::Live,
    auth: (&'static str, &str),
    origin: Option<&str>,
    jmap: bool,
) -> Result<Socket, u16> {
    let url = format!("{}/jmap/ws/", live.base.replacen("http", "ws", 1));
    let mut req = url.into_client_request().expect("request");
    let h = req.headers_mut();
    h.insert(auth.0, HeaderValue::from_str(auth.1).expect("auth"));
    match origin {
        Some(o) => {
            h.insert("origin", HeaderValue::from_str(o).expect("origin"));
        }
        None => {
            h.remove("origin");
        }
    }
    if jmap {
        h.insert("sec-websocket-protocol", HeaderValue::from_static("jmap"));
    }
    match tokio_tungstenite::connect_async(req).await {
        Ok((socket, resp)) => {
            assert_eq!(resp.headers()["sec-websocket-protocol"], "jmap");
            Ok(socket)
        }
        Err(WsError::Http(resp)) => Err(resp.status().as_u16()),
        Err(e) => panic!("upgrade failed without a status: {e}"),
    }
}

/// The next JSON text frame, within `secs`.
async fn next_json(socket: &mut Socket, secs: u64) -> Option<Value> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(left, socket.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => return serde_json::from_str(&t).ok(),
            Ok(Some(Ok(_))) => continue,
            _ => return None,
        }
    }
}

pub(super) async fn websocket(live: &live::Live, alice: &str) {
    let bearer = format!("Bearer {alice}");
    let cookie = format!("mailrs_session={alice}");
    assert_eq!(
        upgrade(live, ("authorization", &bearer), None, false)
            .await
            .err(),
        Some(400),
        "the jmap subprotocol is required"
    );
    assert_eq!(
        upgrade(
            live,
            ("cookie", &cookie),
            Some("https://evil.example"),
            true
        )
        .await
        .err(),
        Some(403),
        "a cookie upgrade from another site"
    );
    assert!(
        upgrade(live, ("cookie", &cookie), Some(ORIGIN), true)
            .await
            .is_ok()
    );
    let Ok(mut ws) = upgrade(live, ("authorization", &bearer), None, true).await else {
        panic!("a bearer upgrade is refused");
    };
    let send = |v: Value| Message::Text(v.to_string().into());
    ws.send(send(json!({
        "@type": "Request", "id": "r1", "using": USING,
        "methodCalls": [["Core/echo", { "hello": 1 }, "0"]]
    })))
    .await
    .expect("send");
    let r = next_json(&mut ws, 10).await.expect("a response");
    assert_eq!(r["@type"], "Response", "{r}");
    assert_eq!(r["requestId"], "r1");
    assert_eq!(r["methodResponses"][0][1]["hello"], 1);
    ws.send(send(json!({ "@type": "Nonsense" })))
        .await
        .expect("send");
    let r = next_json(&mut ws, 10).await.expect("an error");
    assert_eq!(r["@type"], "RequestError", "{r}");

    // push: a change made over HTTP arrives on the socket
    ws.send(send(
        json!({ "@type": "WebSocketPushEnable", "dataTypes": null }),
    ))
    .await
    .expect("send");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let id = import(
        live,
        alice,
        ALICE,
        &message(
            "Pushed over the socket",
            "Friend <friend@partner.example>",
            "hi",
        ),
    )
    .await;
    let mut seen = None;
    while let Some(v) = next_json(&mut ws, 20).await {
        if v["@type"] == "StateChange" {
            seen = Some(v);
            break;
        }
    }
    let v = seen.expect("a StateChange within 20 s");
    assert!(v["changed"][ALICE]["Email"].is_string(), "{v} after {id}");
    assert_eq!(v["pushState"], v["changed"][ALICE]["Email"]);
}

/// The same request over HTTP and over the socket, answered the same way:
/// a request over a limit is refused with the same error (and the socket
/// closed after it), and another account's data is not found on either.
pub(super) async fn limits_alike(live: &live::Live, alice: &str) {
    let bearer = format!("Bearer {alice}");
    let http = |body: Vec<u8>| async move {
        let r = live
            .http
            .post(format!("{}/jmap/api/", live.base))
            .bearer_auth(alice)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .expect("post");
        let status = r.status().as_u16();
        (status, r.json::<Value>().await.unwrap_or(Value::Null))
    };
    let over_ws = |body: Vec<u8>| {
        let bearer = bearer.clone();
        async move {
            let Ok(mut ws) = upgrade(live, ("authorization", &bearer), None, true).await else {
                panic!("upgrade refused");
            };
            ws.send(Message::Text(String::from_utf8(body).unwrap().into()))
                .await
                .expect("send");
            let answer = next_json(&mut ws, 10).await.expect("an answer");
            let mut closed = false;
            while let Ok(Some(m)) = tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
                if matches!(m, Ok(Message::Close(_)) | Err(_)) {
                    closed = true;
                    break;
                }
            }
            (answer, closed)
        }
    };
    let same = |h: &Value, w: &Value, what: &str| {
        for k in ["type", "status", "limit"] {
            assert_eq!(h[k], w[k], "{what}: {k} differs: http {h} ws {w}");
        }
    };

    let calls: Vec<Value> = (0..17)
        .map(|n| json!(["Core/echo", {}, n.to_string()]))
        .collect();
    let too_many = json!({ "@type": "Request", "id": "x", "using": USING, "methodCalls": calls });
    let (status, h) = http(too_many.to_string().into_bytes()).await;
    assert_eq!(status, 400, "{h}");
    assert_eq!(h["limit"], "maxCallsInRequest");
    let (w, closed) = over_ws(too_many.to_string().into_bytes()).await;
    assert_eq!(w["@type"], "RequestError", "{w}");
    same(&h, &w, "too many calls");
    assert!(closed, "the socket stays open after a request over a limit");

    // one byte over maxSizeRequest (10,000,000), the same bytes both ways
    let head = r#"{"@type":"Request","id":"y","using":["urn:ietf:params:jmap:core"],"methodCalls":[],"pad":""#;
    let tail = r#""}"#;
    let pad = 10_000_001 - head.len() - tail.len();
    let big = format!("{head}{}{tail}", "x".repeat(pad)).into_bytes();
    assert_eq!(big.len(), 10_000_001);
    let (status, h) = http(big.clone()).await;
    assert_eq!(status, 400, "{h}");
    assert_eq!(h["limit"], "maxSizeRequest");
    let (w, closed) = over_ws(big).await;
    same(&h, &w, "too large");
    assert!(closed, "the socket stays open after an oversized request");

    // another account named: not found, the same way on both
    let other = json!({ "@type": "Request", "id": "z", "using": USING,
        "methodCalls": [["Email/get", { "accountId": BOB, "ids": ["M1"] }, "0"]] });
    let (status, h) = http(other.to_string().into_bytes()).await;
    assert_eq!(status, 200);
    let Ok(mut ws) = upgrade(live, ("authorization", &bearer), None, true).await else {
        panic!("upgrade refused");
    };
    ws.send(Message::Text(other.to_string().into()))
        .await
        .expect("send");
    let w = next_json(&mut ws, 10).await.expect("answer");
    assert_eq!(
        h["methodResponses"][0], w["methodResponses"][0],
        "http {h} ws {w}"
    );
    assert_eq!(w["methodResponses"][0][1]["type"], "accountNotFound", "{w}");
}

/// A proxy reload drops every socket. The client reconnects and, from the
/// state it held, gets what changed while it was away: by `Email/changes`,
/// and by a push as soon as it says where it was.
pub(super) async fn reconnect_catches_up(live: &live::Live, alice: &str) {
    let bearer = format!("Bearer {alice}");
    let Ok(mut ws) = upgrade(live, ("authorization", &bearer), None, true).await else {
        panic!("upgrade refused");
    };
    ws.send(request(json!([["Email/get", { "ids": [] }, "0"]])))
        .await
        .expect("send");
    let r = next_json(&mut ws, 10).await.expect("answer");
    let held = r["methodResponses"][0][1]["state"]
        .as_str()
        .expect("state")
        .to_string();
    // the connection goes, as a reload takes it
    drop(ws);
    let id = import(
        live,
        alice,
        ALICE,
        &message("While away", "Friend <friend@partner.example>", "missed"),
    )
    .await;

    let Ok(mut ws) = upgrade(live, ("authorization", &bearer), None, true).await else {
        panic!("reconnect refused");
    };
    ws.send(request(
        json!([["Email/changes", { "sinceState": held }, "0"]]),
    ))
    .await
    .expect("send");
    let r = next_json(&mut ws, 10).await.expect("answer");
    let created = &r["methodResponses"][0][1]["created"];
    assert!(
        created.as_array().is_some_and(|c| c.contains(&json!(id))),
        "{r}"
    );
    // and push from where it was: behind, so told at once
    let enable =
        json!({ "@type": "WebSocketPushEnable", "dataTypes": ["Email"], "pushState": held });
    ws.send(Message::Text(enable.to_string().into()))
        .await
        .expect("send");
    let v = next_json(&mut ws, 10).await.expect("a StateChange");
    assert_eq!(v["@type"], "StateChange", "{v}");
    assert_ne!(v["pushState"], json!(held));
}

/// A Request frame carrying `calls`.
fn request(calls: Value) -> Message {
    let frame = json!({ "@type": "Request", "id": "r", "using": USING, "methodCalls": calls });
    Message::Text(frame.to_string().into())
}
