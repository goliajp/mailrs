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

/// A redirect out of our domains is stored only to the address verified
/// for forwarding on the web, and storing one tells the owner.
#[tokio::test]
async fn a_redirect_out_needs_the_verified_address_and_the_owner_is_told() {
    use mailrs_core_sidestate::families::{forwarding, owner_notices};
    let tmp = tempfile::tempdir().unwrap();
    let url = "mem://fastcore-protocols-sieve-redirect";
    let mut kv = kevy_client::Connection::connect(url).unwrap();
    let mut st = state();
    st.net_url = Some(url.into());
    let dir = directory(st, tmp.path());
    let script = "redirect \"me@out.example\";";
    let put = format!("PUTSCRIPT \"mailrs\" {{{}+}}\r\n{script}", script.len());
    let out = conversation(dir.clone(), &[&put]).await;
    assert!(
        out[0].starts_with("NO \"redirect to me@out.example refused"),
        "{out:?}"
    );
    assert!(owner_notices::take(&mut kv, 10).unwrap().is_empty());

    let code = forwarding::request(&mut kv, USER, "me@out.example", true, 1).unwrap();
    forwarding::verify(&mut kv, USER, &code, 2).unwrap();
    let out = conversation(dir, &[&put]).await;
    assert_eq!(out, ["OK \"Script stored\"\r\n"]);
    let told = owner_notices::take(&mut kv, 10).unwrap();
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0].user, USER.to_lowercase());
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
