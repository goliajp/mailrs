//! `classify_recipients` — split forward-path list into local
//! (after alias resolution) and remote recipients.

use crate::ResolvedRecipient;

use super::super::DeliveryDeps;
use super::super::address::is_local_domain;

/// Split `forward_paths` into local (after alias resolution) and
/// remote (with `is_forwarded` flag) recipients, deduping locals
/// case-insensitively. Pure helper: no I/O beyond `account_store`.
pub async fn classify_recipients(
    forward_paths: &[String],
    deps: &DeliveryDeps<'_>,
) -> (Vec<String>, Vec<(String, bool)>) {
    let mut initial_local: Vec<String> = Vec::with_capacity(forward_paths.len());
    let mut remote_rcpts: Vec<(String, bool)> = Vec::with_capacity(forward_paths.len());
    for rcpt in forward_paths {
        if rcpt
            .split_once('@')
            .map(|(_, domain)| is_local_domain(domain, deps.local_domains))
            .unwrap_or(true)
        {
            initial_local.push(rcpt.clone());
        } else {
            remote_rcpts.push((rcpt.clone(), false));
        }
    }

    let mut local_rcpts: Vec<String> = Vec::with_capacity(initial_local.len());
    for rcpt in &initial_local {
        if let Some(ds) = deps.account_store {
            match ds.resolve_recipient(rcpt).await {
                ResolvedRecipient::Account(addr) => {
                    local_rcpts.push(addr);
                }
                ResolvedRecipient::Group(members) => {
                    for m in members {
                        local_rcpts.push(m);
                    }
                }
                ResolvedRecipient::Forward(addrs) => {
                    for a in addrs {
                        if a.split_once('@')
                            .map(|(_, d)| is_local_domain(d, deps.local_domains))
                            .unwrap_or(true)
                        {
                            local_rcpts.push(a);
                        } else {
                            remote_rcpts.push((a, true));
                        }
                    }
                }
                ResolvedRecipient::Reject => {
                    local_rcpts.push(rcpt.to_string());
                }
            }
        } else {
            local_rcpts.push(rcpt.to_string());
        }
    }

    local_rcpts.sort_unstable();
    local_rcpts.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    (local_rcpts, remote_rcpts)
}
