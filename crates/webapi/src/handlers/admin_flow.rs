//! `GET /api/admin/flow`: where mail is in the server right now, stage by
//! stage, for the admin overview. Every number comes from the place that
//! owns it; a stage whose owner cannot be read is `null`, never zero.

use std::path::Path;
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use mailrs_core_sidestate::families::{mock, outbound};
use serde::Serialize;

use crate::WebState;
use crate::handlers::kevy_util::with_kevy;

#[derive(Debug, Serialize)]
pub struct Receiving {
    pub active_connections: u64,
    pub connections: u64,
    /// accepted into the spool since the receiver started
    pub spooled: u64,
    pub uptime_secs: u64,
}

#[derive(Debug, Serialize)]
pub struct Outbound {
    pub queued: u64,
    pub retrying: u64,
    pub scheduled: u64,
    /// failed for good and kept for a look
    pub failed: u64,
}

#[derive(Debug, Serialize)]
pub struct MockFlow {
    pub received: i64,
    pub bytes: i64,
    pub max_bytes: i64,
}

#[derive(Debug, Serialize)]
pub struct Flow {
    pub receiving: Option<Receiving>,
    /// files waiting in the spool for fastcore to deliver
    pub spool_backlog: Option<u64>,
    pub outbound: Option<Outbound>,
    pub mock: Option<MockFlow>,
}

pub async fn flow(State(state): State<Arc<WebState>>) -> Json<Flow> {
    let receiving = state.receiver_stats.get().map(|s| Receiving {
        active_connections: s.active_connections,
        connections: s.total_connections,
        spooled: s.total_messages,
        uptime_secs: s.uptime_secs,
    });
    let spool = std::env::var("MAILRS_SPOOL_ROOT").unwrap_or_else(|_| "/data/.spool".into());
    let spool_backlog = spool_files(Path::new(&spool));
    let outbound = with_kevy(|c| {
        Ok(Outbound {
            queued: c.llen(outbound::PENDING_IDX)? as u64,
            retrying: c.zcard(outbound::RETRY_IDX)? as u64,
            scheduled: c.zcard(outbound::SCHEDULED_IDX)? as u64,
            failed: c.scard(outbound::FAILED_SET)? as u64,
        })
    })
    .ok();
    let mock = match mock::domain() {
        "" => None,
        _ => with_kevy(|c| {
            let stats = mock::stats(c).map_err(std::io::Error::other)?;
            let settings = mock::settings(c).map_err(std::io::Error::other)?;
            Ok(MockFlow {
                received: stats.received,
                bytes: stats.bytes,
                max_bytes: settings.max_total_bytes(),
            })
        })
        .ok(),
    };
    Json(Flow {
        receiving,
        spool_backlog,
        outbound,
        mock,
    })
}

// `None` when the spool cannot be read at all, which is not an empty spool
fn spool_files(root: &Path) -> Option<u64> {
    let mut readable = false;
    let mut count = 0;
    for sub in ["incoming/new", "incoming/cur"] {
        if let Ok(entries) = std::fs::read_dir(root.join(sub)) {
            readable = true;
            count += entries.flatten().filter(|e| e.path().is_file()).count() as u64;
        }
    }
    readable.then_some(count)
}

#[cfg(test)]
mod tests {
    use super::spool_files;

    #[test]
    fn an_unreadable_spool_is_unknown_and_an_empty_one_is_zero() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(spool_files(&tmp.path().join("missing")), None);
        std::fs::create_dir_all(tmp.path().join("incoming/new")).unwrap();
        assert_eq!(spool_files(tmp.path()), Some(0));
        std::fs::write(tmp.path().join("incoming/new/a"), b"x").unwrap();
        std::fs::create_dir_all(tmp.path().join("incoming/cur")).unwrap();
        std::fs::write(tmp.path().join("incoming/cur/b"), b"x").unwrap();
        assert_eq!(spool_files(tmp.path()), Some(2));
    }
}
