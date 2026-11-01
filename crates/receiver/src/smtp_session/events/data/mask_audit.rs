//! Mail submitted from a masked address carries no trace of the client: no
//! IP and no EHLO name in its `Received:` header. What the header leaves
//! out is kept here instead, in the audit log: the account, the mask (its
//! first eight characters, as the mask's other audit rows name it), the
//! client's IP, and the time.

use std::net::IpAddr;

use mailrs_core_sidestate::families::audit;

use super::super::super::ConnectionContext;

pub(super) const ACTION: &str = "mask.submit";

fn target(mask: &str) -> String {
    mask.chars().take(8).collect()
}

fn detail(client: IpAddr) -> String {
    format!("ip={client}")
}

/// Append the row. A store that cannot be reached is logged and the mail
/// goes on: sending comes before its record, as everywhere on this path.
pub(super) async fn record(ctx: &ConnectionContext, user: &str, mask: &str, client: IpAddr) {
    let Some(kevy) = ctx.spam_lists_client.clone() else {
        return;
    };
    let (user, target, detail) = (user.to_string(), target(mask), detail(client));
    let now = chrono::Utc::now().timestamp();
    let written = tokio::task::spawn_blocking(move || {
        kevy.with_conn(|c| audit::append(c, &user, ACTION, &target, &detail, now))
    })
    .await;
    match written {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            tracing::warn!(error = %e, %client, "masked submission: audit row not written")
        }
        Err(e) => tracing::warn!(error = %e, %client, "masked submission: audit task failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_names_the_mask_by_prefix_and_keeps_the_ip() {
        assert_eq!(target("amber.otter.000042@golia.jp"), "amber.ot");
        assert_eq!(detail("203.0.113.9".parse().unwrap()), "ip=203.0.113.9");
    }
}
