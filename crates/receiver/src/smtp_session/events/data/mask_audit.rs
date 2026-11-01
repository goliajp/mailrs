//! Mail submitted from a masked address carries no trace of the client: no
//! IP and no EHLO name in its `Received:` header. What the header leaves
//! out is kept in the masks' own submission log instead
//! (`masked_email::submit_log`): the account, the mask (its first eight
//! characters, as the mask's audit rows name it), the client's IP, and the
//! time.

use std::net::IpAddr;

use mailrs_core_sidestate::families::masked_email::submit_log::{self, Submission};

use super::super::super::ConnectionContext;

fn row(user: &str, mask: &str, client: IpAddr, at: i64) -> Submission {
    Submission {
        at,
        account: user.to_string(),
        mask: mask.chars().take(8).collect(),
        ip: client.to_string(),
    }
}

/// Append the row. A store that cannot be reached is counted, logged, and
/// the mail goes on: sending comes before its record, as everywhere on
/// this path.
pub(super) async fn record(ctx: &ConnectionContext, user: &str, mask: &str, client: IpAddr) {
    let Some(kevy) = ctx.spam_lists_client.clone() else {
        ctx.metrics.mask_submit_log_failed();
        tracing::warn!(%client, "masked submission: no store to log it in");
        return;
    };
    let row = row(user, mask, client, chrono::Utc::now().timestamp());
    let written =
        tokio::task::spawn_blocking(move || kevy.with_conn(|c| submit_log::append(c, &row))).await;
    let error = match written {
        Ok(Ok(())) => return,
        Ok(Err(e)) => e.to_string(),
        Err(e) => e.to_string(),
    };
    ctx.metrics.mask_submit_log_failed();
    tracing::warn!(%error, %client, "masked submission: log row not written");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_names_the_mask_by_prefix_and_keeps_the_ip() {
        let r = row(
            "me@golia.jp",
            "amber.otter.000042@golia.jp",
            "203.0.113.9".parse().unwrap(),
            7,
        );
        assert_eq!(r.mask, "amber.ot");
        assert_eq!(r.ip, "203.0.113.9");
        assert_eq!((r.account.as_str(), r.at), ("me@golia.jp", 7));
    }
}
