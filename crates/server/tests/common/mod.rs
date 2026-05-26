//! Shared helpers for e2e_* integration tests.
//!
//! Each `tests/e2e_*.rs` file is its own test binary; this module
//! lives under `tests/common/` (the standard "shared test code, not
//! a test binary" convention) and is loaded via `mod common;` at
//! the top of each test binary.

#![allow(dead_code)]

pub mod auth_mock;
pub mod imap_mock;
pub mod smtp_mock;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

pub async fn connect(
    port: u16,
) -> (
    BufReader<tokio::net::tcp::OwnedReadHalf>,
    tokio::net::tcp::OwnedWriteHalf,
) {
    let stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    let (read, write) = stream.into_split();
    (BufReader::new(read), write)
}

pub async fn read_line(reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> String {
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    line
}

pub async fn read_multiline(reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> String {
    let mut result = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let cont = line.len() >= 4 && line.as_bytes()[3] == b'-';
        result.push_str(&line);
        if !cont {
            break;
        }
    }
    result
}

pub async fn send(writer: &mut tokio::net::tcp::OwnedWriteHalf, cmd: &str) {
    writer
        .write_all(format!("{cmd}\r\n").as_bytes())
        .await
        .unwrap();
}
