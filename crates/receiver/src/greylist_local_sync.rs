//! Kevy-backed loader for the admin-managed local greylist lists.
//!
//! The admin UI (webapi) writes entries into the shared network kevy
//! at hash `admin:greylist:local-lists` — field = numeric id, value =
//! JSON `{ id, address_or_domain, list_type: "whitelist"|"blacklist",
//! created_at }`. Until 2026-07-04 nothing in the receiver ever read
//! that hash (the loader was monolith/PG-side), so admin whitelist
//! entries had ZERO effect on the greylist stage. This module closes
//! that: fetch → classify → swap the `GreylistLocalLists` snapshot,
//! at boot and every `interval_secs`.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use ipnet::IpNet;

use crate::greylist_local::{GreylistLocalHandle, GreylistLocalLists};
use crate::kevy_net::KevyNetClient;

const GL_KEY: &[u8] = b"admin:greylist:local-lists";

/// Classify one admin entry value into the right snapshot bucket.
/// `address_or_domain` is free-form admin input: an email (has `@`),
/// a CIDR (parses as `IpNet`), or a domain (everything else).
fn classify(lists: &mut GreylistLocalLists, value: &str, list_type: &str) {
    let v = value.trim().to_lowercase();
    if v.is_empty() {
        return;
    }
    let white = matches!(list_type, "whitelist" | "white");
    if v.contains('@') {
        if white {
            lists.white_emails.insert(v);
        } else {
            lists.black_emails.insert(v);
        }
    } else if let Ok(net) = IpNet::from_str(&v) {
        if white {
            lists.white_cidrs.push(net);
        } else {
            lists.black_cidrs.push(net);
        }
    } else if white {
        lists.white_domains.insert(v);
    } else {
        lists.black_domains.insert(v);
    }
}

/// Build a snapshot from the raw HGETALL result (flat `[k, v, k, v]`).
fn parse_entries(flat: &[Vec<u8>]) -> GreylistLocalLists {
    let mut lists = GreylistLocalLists::default();
    for pair in flat.chunks(2) {
        let [_field, value] = pair else { continue };
        let Ok(entry) = serde_json::from_slice::<serde_json::Value>(value) else {
            continue;
        };
        let addr = entry
            .get("address_or_domain")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let list_type = entry
            .get("list_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        classify(&mut lists, addr, list_type);
    }
    lists
}

async fn reload_once(handle: &GreylistLocalHandle, client: &Arc<KevyNetClient>) {
    let c = client.clone();
    let flat = tokio::task::spawn_blocking(move || c.with_conn(|conn| conn.hgetall(GL_KEY)))
        .await
        .ok()
        .and_then(Result::ok);
    let Some(flat) = flat else {
        // kevy unreachable — keep the previous snapshot (fail-open:
        // stale lists beat no lists)
        metrics::counter!("mailrs_greylist_local_sync_total", "outcome" => "error").increment(1);
        return;
    };
    let mut lists = parse_entries(&flat);
    lists.last_reload_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs());
    let white = lists.white_count();
    let black = lists.black_count();
    *handle.write().await = lists;
    metrics::counter!("mailrs_greylist_local_sync_total", "outcome" => "ok").increment(1);
    metrics::gauge!("mailrs_greylist_local_white_size").set(white as f64);
    metrics::gauge!("mailrs_greylist_local_black_size").set(black as f64);
    tracing::debug!(target: "greylist.sync", white, black, "local greylist lists reloaded");
}

/// Spawn the reload task: once at boot, then every `interval_secs`.
pub fn spawn_reload_task(
    handle: GreylistLocalHandle,
    client: Arc<KevyNetClient>,
    interval_secs: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        reload_once(&handle, &client).await;
        let mut tick = tokio::time::interval(Duration::from_secs(interval_secs));
        tick.tick().await;
        loop {
            tick.tick().await;
            reload_once(&handle, &client).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(addr: &str, list_type: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "id": 1, "address_or_domain": addr, "list_type": list_type,
            "created_at": 0,
        }))
        .unwrap()
    }

    #[test]
    fn classifies_email_domain_cidr_into_buckets() {
        let flat = vec![
            b"1".to_vec(),
            entry("Boss@Partner.com", "whitelist"),
            b"2".to_vec(),
            entry("partner.com", "whitelist"),
            b"3".to_vec(),
            entry("10.0.0.0/8", "blacklist"),
            b"4".to_vec(),
            entry("spammer.example", "blacklist"),
        ];
        let l = parse_entries(&flat);
        assert!(l.white_emails.contains("boss@partner.com"));
        assert!(l.white_domains.contains("partner.com"));
        assert_eq!(l.black_cidrs.len(), 1);
        assert!(l.black_domains.contains("spammer.example"));
        assert_eq!(l.white_count(), 2);
        assert_eq!(l.black_count(), 2);
    }

    #[test]
    fn garbage_entries_are_skipped() {
        let flat = vec![
            b"1".to_vec(),
            b"not json".to_vec(),
            b"2".to_vec(),
            entry("", "whitelist"),
            b"3".to_vec(),
            entry("ok.example", "whitelist"),
        ];
        let l = parse_entries(&flat);
        assert_eq!(l.total(), 1);
        assert!(l.white_domains.contains("ok.example"));
    }
}
