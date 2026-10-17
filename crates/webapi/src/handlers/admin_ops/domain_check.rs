//! POST /api/admin/domains/{name}/check — MX, SPF, DKIM and DMARC for one
//! domain, as the list of checks the domain page renders.
//!
//! This half asks DNS; `report.rs` judges what came back.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

mod report;
use report::{DomainCheckReport, Lookup, Lookups, build_report};

fn from_result<T>(
    r: Result<T, hickory_resolver::net::NetError>,
    f: impl FnOnce(T) -> Vec<String>,
) -> Lookup {
    match r {
        Ok(v) => Lookup::Found(f(v)),
        Err(e) if e.is_no_records_found() => Lookup::Missing,
        Err(e) => Lookup::Failed(e.to_string()),
    }
}

async fn txt(r: &hickory_resolver::TokioResolver, name: &str) -> Lookup {
    from_result(r.txt_lookup(name).await, |l| {
        l.answers()
            .iter()
            .filter_map(|record| match &record.data {
                hickory_resolver::proto::rr::RData::TXT(txt) => Some(txt.to_string()),
                _ => None,
            })
            .collect()
    })
}

pub async fn check_domain_dns(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(name): Path<String>,
) -> Result<Json<DomainCheckReport>, StatusCode> {
    let resolver = hickory_resolver::TokioResolver::builder_tokio()
        .and_then(|b| b.build())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // the selector the sender signs with; webapi needs it set as well
    let dkim_selector = std::env::var("MAILRS_DKIM_SELECTOR")
        .ok()
        .filter(|s| !s.is_empty());
    let dkim = match &dkim_selector {
        Some(s) => txt(&resolver, &format!("{s}._domainkey.{name}")).await,
        None => Lookup::NotAsked("MAILRS_DKIM_SELECTOR is not set on webapi".into()),
    };
    let mx = from_result(resolver.mx_lookup(&name).await, |l| {
        l.answers()
            .iter()
            .filter_map(|record| match &record.data {
                hickory_resolver::proto::rr::RData::MX(mx) => Some(format!(
                    "{} {}",
                    mx.preference,
                    mx.exchange.to_utf8().trim_end_matches('.')
                )),
                _ => None,
            })
            .collect()
    });
    let lookups = Lookups {
        mx,
        spf: txt(&resolver, &name).await,
        dkim_selector,
        dkim,
        dmarc: txt(&resolver, &format!("_dmarc.{name}")).await,
    };
    let now = crate::handlers::complete::now_secs();
    Ok(Json(build_report(&name, now, &lookups)))
}
