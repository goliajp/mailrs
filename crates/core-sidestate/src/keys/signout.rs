//! Ending an account's sign-ins across processes: webapi publishes the
//! account on [`CHANNEL`] when its password is cleared, and every process
//! that holds signed-in connections (fastcore's IMAP, POP3 and ManageSieve,
//! the receiver's SMTP submission) closes the ones it has for that account.

use std::time::Duration;

use kevy_client::Connection;

/// The pub/sub channel; the message is the account's address.
pub const CHANNEL: &[u8] = b"notify:account-signout";

/// Tell every process to close `address`'s signed-in connections.
pub fn publish(c: &mut Connection, address: &str) -> std::io::Result<()> {
    c.publish(CHANNEL, address.as_bytes())
        .map(|_| ())
        .map_err(std::io::Error::from)
}

/// Call `end` with each address published, on a thread of its own, for as
/// long as the process runs. A dropped subscription is made again after a
/// second; a sign-out published in that second reaches nobody, which the
/// password cleared before it makes harmless: no new sign-in succeeds.
pub fn listen<F>(kevy_url: String, end: F)
where
    F: Fn(&str) + Send + 'static,
{
    std::thread::Builder::new()
        .name("account-signout".into())
        .spawn(move || {
            loop {
                match kevy_client::Subscriber::connect_channels(&kevy_url, &[CHANNEL]) {
                    Ok(mut sub) => {
                        tracing::info!(
                            "signed-in connections close when an account's sign-ins end"
                        );
                        while let Ok((_, payload)) = sub.recv_message() {
                            if let Ok(address) = std::str::from_utf8(&payload) {
                                end(address.trim());
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(err = %e, "account sign-out subscribe failed; retry 1s")
                    }
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        })
        .expect("spawn account-signout thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_channel_keeps_its_spelling() {
        assert_eq!(CHANNEL, b"notify:account-signout");
    }
}
