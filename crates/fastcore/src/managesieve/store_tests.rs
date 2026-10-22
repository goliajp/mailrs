//! A script store that cannot be reached is reported as such, never as a
//! missing or deleted script.

use std::sync::Arc;

use base64::Engine;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::FastcoreState;
use crate::tls::test_support::{PASSWORD, USER, state};

async fn conversation(state: Arc<FastcoreState>, lines: &[&str]) -> Vec<String> {
    let (client, server) = tokio::io::duplex(1 << 16);
    tokio::spawn(super::session(state, server, None));
    let (r, mut w) = tokio::io::split(client);
    let mut r = BufReader::new(r);
    let mut l = String::new();
    while !l.starts_with("OK") {
        l.clear();
        r.read_line(&mut l).await.unwrap();
    }
    let ir = base64::engine::general_purpose::STANDARD.encode(format!("\0{USER}\0{PASSWORD}"));
    w.write_all(format!("AUTHENTICATE \"PLAIN\" \"{ir}\"\r\n").as_bytes())
        .await
        .unwrap();
    l.clear();
    r.read_line(&mut l).await.unwrap();
    assert!(l.starts_with("OK"), "{l}");
    let mut out = Vec::new();
    for line in lines {
        w.write_all(format!("{line}\r\n").as_bytes()).await.unwrap();
        let mut reply = String::new();
        while !(reply.ends_with("\r\n") && (l.starts_with("OK") || l.starts_with("NO"))) {
            l.clear();
            r.read_line(&mut l).await.unwrap();
            reply.push_str(&l);
        }
        out.push(reply);
    }
    out
}

#[tokio::test]
async fn an_unreachable_store_is_not_an_empty_one() {
    let st = state();
    assert!(st.net_url.is_none());
    let out = conversation(
        st,
        &[
            "LISTSCRIPTS",
            "GETSCRIPT \"mailrs\"",
            "DELETESCRIPT \"mailrs\"",
        ],
    )
    .await;
    for reply in out {
        assert!(reply.starts_with("NO (TRYLATER) "), "{reply}");
    }
}

#[tokio::test]
async fn a_reachable_store_keeps_the_script() {
    let kv = kevy_client::Connection::connect("mem://managesieve-store-tests").unwrap();
    let mut st = Arc::into_inner(state()).unwrap();
    st.net_url = Some("mem://managesieve-store-tests".into());
    let out = conversation(
        Arc::new(st),
        &[
            "PUTSCRIPT \"mailrs\" {5+}\r\nkeep;",
            "LISTSCRIPTS",
            "DELETESCRIPT \"mailrs\"",
        ],
    )
    .await;
    assert_eq!(out[0], "OK \"Script stored\"\r\n");
    assert_eq!(
        out[1],
        "\"mailrs\" ACTIVE\r\nOK \"Listscripts completed\"\r\n"
    );
    assert_eq!(out[2], "OK \"Deleted\"\r\n");
    drop(kv);
}
