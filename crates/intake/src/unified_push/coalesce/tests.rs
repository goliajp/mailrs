use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::*;

const SHORT: Duration = Duration::from_millis(300);

fn conn(url: &str) -> kevy_client::Connection {
    kevy_client::Connection::connect(url).unwrap()
}

#[test]
fn a_burst_leads_once_trails_once_and_the_rest_are_covered() {
    let mut c = conn("mem://upush-claim");
    assert_eq!(claim(&mut c, "Me@x", SHORT).unwrap(), Turn::Lead);
    assert!(matches!(
        claim(&mut c, "me@x", SHORT).unwrap(),
        Turn::Trail(w) if w <= SHORT && w > Duration::ZERO
    ));
    assert_eq!(claim(&mut c, "me@x", SHORT).unwrap(), Turn::Covered);
    assert_eq!(claim(&mut c, "me@x", SHORT).unwrap(), Turn::Covered);
    assert_eq!(
        claim(&mut c, "other@x", SHORT).unwrap(),
        Turn::Lead,
        "one window per account"
    );

    // the trailing ping opens the next window: mail right after it trails
    open_next(&mut c, "me@x", SHORT).unwrap();
    assert!(matches!(
        claim(&mut c, "me@x", SHORT).unwrap(),
        Turn::Trail(_)
    ));
    assert_eq!(claim(&mut c, "me@x", SHORT).unwrap(), Turn::Covered);
}

#[test]
fn a_window_left_without_an_expiry_still_closes() {
    let mut c = conn("mem://upush-orphan");
    // a process died between the increment and the expiry
    c.incr(unified_push_window_key("me@x").as_bytes()).unwrap();
    c.incr(unified_push_trailing_key("me@x").as_bytes())
        .unwrap();
    assert_eq!(claim(&mut c, "me@x", SHORT).unwrap(), Turn::Covered);
    std::thread::sleep(SHORT + TRAILING_SLACK + Duration::from_millis(100));
    assert_eq!(claim(&mut c, "me@x", SHORT).unwrap(), Turn::Lead);
}

/// When a ping went out, and the messages a refetch then would see.
type Pings = Vec<(Instant, Vec<u32>)>;

/// Deliveries land in a mailbox; each ping records what a refetch would see.
struct Phone {
    mailbox: Arc<Mutex<Vec<u32>>>,
    pings: Arc<Mutex<Pings>>,
}

impl Phone {
    fn new() -> Self {
        Self {
            mailbox: Arc::default(),
            pings: Arc::default(),
        }
    }

    /// Store message `n`, then do what delivery does about pinging.
    fn deliver(&self, url: &'static str, n: u32) -> tokio::task::JoinHandle<()> {
        self.mailbox.lock().unwrap().push(n);
        let (mailbox, pings) = (self.mailbox.clone(), self.pings.clone());
        tokio::spawn(async move {
            if gate(move || Ok(conn(url)), "me@x", SHORT).await.unwrap() {
                let seen = mailbox.lock().unwrap().clone();
                pings.lock().unwrap().push((Instant::now(), seen));
            }
        })
    }
}

#[tokio::test]
async fn the_last_mail_of_a_burst_is_always_pinged_for() {
    const URL: &str = "mem://upush-burst";
    // a mem:// store lives only while a connection holds it, and each
    // delivery connects afresh, as it does against the network kevy
    let _store = conn(URL);
    let phone = Phone::new();
    let mut tasks = vec![phone.deliver(URL, 1)];
    tokio::time::sleep(Duration::from_millis(50)).await;
    tasks.push(phone.deliver(URL, 2));
    tokio::time::sleep(Duration::from_millis(50)).await;
    tasks.push(phone.deliver(URL, 3));
    for t in tasks {
        t.await.unwrap();
    }
    {
        let pings = phone.pings.lock().unwrap();
        assert_eq!(pings.len(), 2, "one leading and one trailing ping");
        assert_eq!(pings[1].1, vec![1, 2, 3], "the trailing refetch sees all");
        let gap = pings[1].0 - pings[0].0;
        assert!(gap >= SHORT - Duration::from_millis(150), "{gap:?}");
    }

    // once the window the trailing ping opened has closed, mail pings at once
    tokio::time::sleep(SHORT + Duration::from_millis(100)).await;
    let before = Instant::now();
    phone.deliver(URL, 4).await.unwrap();
    let pings = phone.pings.lock().unwrap();
    assert_eq!(pings.len(), 3);
    assert_eq!(pings[2].1, vec![1, 2, 3, 4]);
    assert!(
        pings[2].0 - before < SHORT / 2,
        "a quiet account pings at once"
    );
}
