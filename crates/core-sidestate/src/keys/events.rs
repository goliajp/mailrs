//! Live-update frames: every change the web clients should hear of is
//! published on one channel, and the webapi's subscriber forwards each frame
//! to the sockets of the user it names. A channel rather than keys a change
//! feed reads: PUBLISH and SUBSCRIBE are what any Redis-compatible server
//! has, and a frame nobody is subscribed for is not missed, because a client
//! whose socket reconnects refetches everything.

/// The channel every frame is published on.
pub const EVENTS_CHANNEL: &[u8] = b"mailrs:events";

/// Publish one frame, a JSON object naming the `user` it is for.
pub fn send(conn: &mut kevy_client::Connection, frame: &[u8]) -> kevy_client::KevyResult<()> {
    conn.publish(EVENTS_CHANNEL, frame).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_channel_keeps_its_prod_spelling() {
        assert_eq!(EVENTS_CHANNEL, b"mailrs:events");
    }
}
