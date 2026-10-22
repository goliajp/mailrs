//! A self-signed certificate on both ends, a line client that can switch
//! to TLS mid-session, and an in-memory directory holding one account.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::client::TlsStream;

pub(crate) use crate::test_tls::{Cert, cert};

use crate::backend::{
    Account, Directory, Error, Expunged, Filed, Flag, Message, Protocol, Quota, Result, Selected,
    Watch,
};

pub(crate) const USER: &str = "alice@golia.example.test";
pub(crate) const PASSWORD: &str = "correct horse";

/// An INBOX in memory: `(uid, flags, bytes)`.
type Inbox = Arc<Mutex<Vec<(u32, Vec<Flag>, Vec<u8>)>>>;

/// One account and its INBOX.
#[derive(Default)]
pub(crate) struct Memory {
    inbox: Inbox,
    script: Arc<Mutex<Option<String>>>,
}

pub(crate) fn directory() -> Arc<Memory> {
    Arc::new(Memory::default())
}

impl Memory {
    pub(crate) fn deliver(&self, raw: &str) {
        file(&self.inbox, raw.as_bytes().to_vec(), Vec::new());
    }
}

fn file(inbox: &Inbox, bytes: Vec<u8>, flags: Vec<Flag>) -> Filed {
    let mut inbox = inbox.lock().unwrap();
    let uid = inbox.last().map_or(0, |m| m.0) + 1;
    inbox.push((uid, flags, bytes));
    Filed {
        uidvalidity: 7,
        uid,
    }
}

impl Directory for Memory {
    type Account = MemoryAccount;

    fn hostname(&self) -> String {
        "mail.example.test".into()
    }

    async fn login(
        &self,
        user: &str,
        password: &str,
        _: Protocol,
    ) -> Result<Option<MemoryAccount>> {
        Ok(
            (user == USER && password == PASSWORD).then(|| MemoryAccount {
                inbox: self.inbox.clone(),
                script: self.script.clone(),
            }),
        )
    }
}

pub(crate) struct MemoryAccount {
    inbox: Inbox,
    script: Arc<Mutex<Option<String>>>,
}

pub(crate) struct Never;

impl Watch for Never {
    async fn changed(&mut self) {
        std::future::pending::<()>().await
    }
}

impl MemoryAccount {
    fn listing(&self) -> Vec<Message<u32>> {
        let inbox = self.inbox.lock().unwrap();
        (1u32..)
            .zip(inbox.iter())
            .map(|(seqno, (uid, flags, bytes))| Message {
                uid: *uid,
                seqno,
                flags: flags.clone(),
                internal_date: 0,
                size: bytes.len() as u64,
                modseq: 1,
                key: *uid,
            })
            .collect()
    }

    fn drop_uid(&self, uid: u32) {
        self.inbox.lock().unwrap().retain(|m| m.0 != uid);
    }
}

impl Account for MemoryAccount {
    type Mailbox = ();
    type Key = u32;
    type Watch = Never;

    fn user(&self) -> &str {
        USER
    }

    async fn mailbox_names(&self) -> Result<Vec<String>> {
        Ok(vec!["INBOX".into()])
    }

    async fn mailbox(&self, name: &str) -> Result<Option<()>> {
        Ok(name.eq_ignore_ascii_case("INBOX").then_some(()))
    }

    fn mailbox_name<'a>(&self, _: &'a ()) -> &'a str {
        "INBOX"
    }

    async fn select(&self, _: &()) -> Result<Selected<u32>> {
        let messages = self.listing();
        Ok(Selected {
            uidvalidity: 7,
            uidnext: messages.last().map_or(1, |m| m.uid + 1),
            highest_modseq: 1,
            recent: messages.len() as u32,
            messages,
        })
    }

    async fn messages(&self, _: &()) -> Result<Vec<Message<u32>>> {
        Ok(self.listing())
    }

    async fn read(&self, m: &Message<u32>) -> Result<Option<Vec<u8>>> {
        let inbox = self.inbox.lock().unwrap();
        Ok(inbox.iter().find(|x| x.0 == m.key).map(|x| x.2.clone()))
    }

    async fn store_flags(&self, _: &(), m: &Message<u32>, flags: &[Flag]) -> Result<()> {
        let mut inbox = self.inbox.lock().unwrap();
        let row = inbox.iter_mut().find(|x| x.0 == m.key);
        row.ok_or_else(|| Error::Store("gone".into()))?.1 = flags.to_vec();
        Ok(())
    }

    async fn copy(&self, _: &(), m: &Message<u32>, _: &()) -> Result<Filed> {
        let bytes = self.read(m).await?.unwrap_or_default();
        Ok(file(&self.inbox, bytes, m.flags.clone()))
    }

    async fn move_to(&self, _: &(), m: &Message<u32>, _: &()) -> Result<Filed> {
        let uid = m.uid;
        Ok(Filed {
            uidvalidity: 7,
            uid,
        })
    }

    async fn expunge(&self, _: &(), doomed: &[Message<u32>]) -> Expunged {
        for m in doomed {
            self.drop_uid(m.uid);
        }
        Expunged {
            removed: doomed.iter().map(|m| m.uid).collect(),
            failed: None,
        }
    }

    fn unique_id(&self, m: &Message<u32>) -> String {
        format!("u{}", m.uid)
    }

    async fn remove(&self, _: &(), m: &Message<u32>) -> Result<()> {
        self.drop_uid(m.uid);
        Ok(())
    }

    async fn append(&self, _: &(), bytes: Vec<u8>, flags: Vec<Flag>) -> Result<Filed> {
        Ok(file(&self.inbox, bytes, flags))
    }

    async fn vanished_since(&self, _: &(), _: u64) -> Result<Vec<u32>> {
        Ok(Vec::new())
    }

    async fn quota(&self) -> Result<Quota> {
        Ok(Quota { used: 0, limit: 0 })
    }

    fn watch(&self, _: &()) -> Never {
        Never
    }

    async fn sieve_script(&self) -> Result<Option<String>> {
        Ok(self.script.lock().unwrap().clone())
    }

    async fn put_sieve_script(&self, body: &str) -> Result<()> {
        *self.script.lock().unwrap() = Some(body.to_string());
        Ok(())
    }

    async fn delete_sieve_script(&self) -> Result<()> {
        *self.script.lock().unwrap() = None;
        Ok(())
    }
}

pub(crate) async fn listener() -> (TcpListener, std::net::SocketAddr) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    (l, addr)
}

pub(crate) struct Client<S> {
    io: BufReader<S>,
}

impl Client<TcpStream> {
    pub(crate) async fn connect(addr: std::net::SocketAddr) -> Self {
        Self {
            io: BufReader::new(TcpStream::connect(addr).await.unwrap()),
        }
    }

    /// The handshake, on whatever the server has not answered yet.
    pub(crate) async fn upgrade(self, cert: &Cert) -> Client<TlsStream<TcpStream>> {
        assert!(self.io.buffer().is_empty(), "server spoke before TLS");
        let name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
        let tls = cert
            .connector
            .connect(name, self.io.into_inner())
            .await
            .unwrap();
        Client {
            io: BufReader::new(tls),
        }
    }
}

impl Client<TlsStream<TcpStream>> {
    pub(crate) async fn connect_tls(addr: std::net::SocketAddr, cert: &Cert) -> Self {
        Client::connect(addr).await.upgrade(cert).await
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> Client<S> {
    pub(crate) async fn send(&mut self, raw: &str) {
        self.io.get_mut().write_all(raw.as_bytes()).await.unwrap();
        self.io.get_mut().flush().await.unwrap();
    }

    pub(crate) async fn line(&mut self) -> String {
        let mut l = String::new();
        self.io.read_line(&mut l).await.unwrap();
        assert!(!l.is_empty(), "connection closed");
        l
    }

    /// Whether the server closed the connection without sending more.
    pub(crate) async fn eof(&mut self) -> bool {
        let mut l = String::new();
        self.io.read_line(&mut l).await.is_ok_and(|n| n == 0)
    }

    /// Lines up to and including the first one starting with `end`.
    pub(crate) async fn until(&mut self, end: &str) -> String {
        let mut out = String::new();
        loop {
            let l = self.line().await;
            out.push_str(&l);
            if l.starts_with(end) {
                return out;
            }
        }
    }
}
