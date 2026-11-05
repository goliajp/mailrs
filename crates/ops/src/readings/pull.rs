//! Reading fastcore and webapi directly, five seconds each. No answer in
//! time is "could not read", never an abnormal value.

use std::future::Future;
use std::time::Duration;

use mailrs_core_api::client::Client;
use mailrs_core_api::method::ops::OpsStatusWire;
use mailrs_core_api::types::HealthResponse;

use super::Reading;

pub const TIMEOUT: Duration = Duration::from_secs(5);

pub struct Puller {
    /// Its bearer is the daemon's own token: `/v1/ops/status` asks for it,
    /// health asks for nothing.
    fastcore: Client,
    webapi_url: String,
    http: reqwest::Client,
}

async fn within<T, E: std::fmt::Display>(f: impl Future<Output = Result<T, E>>) -> Reading<T> {
    match tokio::time::timeout(TIMEOUT, f).await {
        Ok(Ok(v)) => Reading::Got(v),
        Ok(Err(e)) => Reading::Unreadable(e.to_string()),
        Err(_) => Reading::Unreadable(format!("no answer within {}s", TIMEOUT.as_secs())),
    }
}

impl Puller {
    pub fn new(fastcore_url: &str, ops_token: Option<&str>, webapi_url: &str) -> Self {
        Puller {
            fastcore: Client::new(fastcore_url.trim_end_matches('/'), ops_token.unwrap_or("")),
            webapi_url: webapi_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    pub async fn fastcore(
        &self,
    ) -> (
        Reading<HealthResponse>,
        Reading<HealthResponse>,
        Reading<OpsStatusWire>,
    ) {
        tokio::join!(
            within(self.fastcore.healthz()),
            within(self.fastcore.readyz()),
            within(self.fastcore.ops_status()),
        )
    }

    /// `/api/health` as JSON, and whether `/api/readiness` says ready (a 503
    /// is an answer: not ready).
    pub async fn webapi(&self) -> (Reading<serde_json::Value>, Reading<bool>) {
        let health = async {
            let resp = self
                .http
                .get(format!("{}/api/health", self.webapi_url))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            match resp.status().is_success() {
                true => resp
                    .json::<serde_json::Value>()
                    .await
                    .map_err(|e| e.to_string()),
                false => Err(format!("answered {}", resp.status())),
            }
        };
        let ready = async {
            let resp = self
                .http
                .get(format!("{}/api/readiness", self.webapi_url))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            Ok::<bool, String>(resp.status().is_success())
        };
        tokio::join!(within(health), within(ready))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A core that takes six seconds to answer is "could not read", not a
    /// value: the reading is given up at five.
    #[tokio::test]
    async fn a_slow_core_is_unreadable_not_abnormal() {
        let app = axum::Router::new().route(
            "/v1/healthz",
            axum::routing::get(|| async {
                tokio::time::sleep(Duration::from_secs(6)).await;
                "late"
            }),
        );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await });
        let p = Puller::new(&format!("http://{addr}"), None, "http://127.0.0.1:9");
        let started = std::time::Instant::now();
        let (health, _, _) = p.fastcore().await;
        assert!(started.elapsed() < Duration::from_millis(5800));
        match health {
            Reading::Unreadable(why) => assert!(why.contains("within 5s"), "{why}"),
            Reading::Got(h) => panic!("read {h:?}"),
        }
    }
}
