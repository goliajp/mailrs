//! A script store that cannot be reached is reported as such, never as a
//! missing or deleted script.

use std::sync::Arc;

use base64::Engine;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::{PASSWORD, USER, directory, state};
use crate::protocols::FastcoreDirectory;

async fn conversation(dir: Arc<FastcoreDirectory>, lines: &[&str]) -> Vec<String> {
    let (client, server) = tokio::io::duplex(1 << 16);
    tokio::spawn(mailrs_mail_protocols::managesieve::session(
        dir, server, None,
    ));
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
        loop {
            l.clear();
            r.read_line(&mut l).await.unwrap();
            reply.push_str(&l);
            if l.starts_with("OK") || l.starts_with("NO") {
                break;
            }
        }
        out.push(reply);
    }
    out
}

#[tokio::test]
async fn an_unreachable_store_is_not_an_empty_one() {
    let tmp = tempfile::tempdir().unwrap();
    let mut st = state();
    st.net_url = None;
    let out = conversation(
        directory(st, tmp.path()),
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
    let tmp = tempfile::tempdir().unwrap();
    let url = "mem://fastcore-protocols-sieve";
    let _kv = kevy_client::Connection::connect(url).unwrap();
    let mut st = state();
    st.net_url = Some(url.into());
    let out = conversation(
        directory(st, tmp.path()),
        &[
            "PUTSCRIPT \"mailrs\" {5+}\r\nkeep;",
            "LISTSCRIPTS",
            "GETSCRIPT \"mailrs\"",
            "DELETESCRIPT \"mailrs\"",
            "LISTSCRIPTS",
        ],
    )
    .await;
    assert_eq!(
        out,
        [
            "OK \"Script stored\"\r\n",
            "\"mailrs\" ACTIVE\r\nOK \"Listscripts completed\"\r\n",
            "{5}\r\nkeep;\r\nOK\r\n",
            "OK \"Deleted\"\r\n",
            "OK \"Listscripts completed\"\r\n",
        ]
    );
}
