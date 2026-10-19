//! A real fastcore and the whole webapi router, both on loopback, sharing
//! one in-process network kevy. Shared by the tests that go through the
//! front door: sign in, call REST and MCP over HTTP, read what came back.

// each test binary that includes this uses a different subset of it
#![allow(dead_code)]

use std::sync::Arc;

use mailrs_core_api::client::Client;
use mailrs_core_api::method::admin as adm;
use mailrs_webapi::WebState;

pub struct Live {
    pub core: Client,
    /// fastcore's own store, for seeding what no REST route writes
    pub mailbox: mailrs_mailbox_kevy::KevyMailboxStore,
    pub base: String,
    pub http: reqwest::Client,
    /// the router's state, for calling a handler without signing in
    pub state: Arc<WebState>,
}

/// Start both servers. `kevy` must already be in `MAILRS_KEVY_URL`.
pub async fn start(kevy: &str) -> Live {
    let store =
        Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"));
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(store);
    let mut fc = mailrs_fastcore::FastcoreState::new(mailbox.clone());
    fc.net_url = Some(kevy.to_string());
    let core_url = serve(mailrs_fastcore::build_router(Arc::new(fc))).await;

    let state = Arc::new(WebState {
        core: Arc::new(Client::new(core_url.clone(), "secret")),
        bind_addr: "127.0.0.1:0".into(),
        event_bus: std::sync::OnceLock::new(),
        trace_bus: std::sync::OnceLock::new(),
        receiver_stats: Arc::new(mailrs_webapi::handlers::smtp_monitor::StatsCache::default()),
        started_at: std::time::Instant::now(),
        llm_config: None,
    });
    let base = serve(mailrs_webapi::build_router(state.clone())).await;
    Live {
        core: Client::new(core_url, "secret"),
        mailbox,
        base,
        http: reqwest::Client::new(),
        state,
    }
}

async fn serve(router: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, router).await });
    format!("http://{addr}")
}

impl Live {
    pub async fn add_account(&self, address: &str, password: &str) {
        self.core
            .add_account(&adm::AddAccountRequest {
                address: address.into(),
                display_name: String::new(),
                password: password.into(),
            })
            .await
            .expect("add account");
    }

    /// A permission group holding `permission`, with `address` in it.
    pub async fn grant(&self, address: &str, permission: &str) {
        use mailrs_webapi::handlers::groups;
        let axum::Json(g) = groups::create_group(axum::Json(groups::CreateGroupRequest {
            name: format!("{permission}-holders"),
            description: String::new(),
        }))
        .await
        .expect("create group");
        let id = g["id"].as_i64().expect("id");
        groups::set_group_permissions(
            axum::extract::Path(id),
            axum::Json(groups::SetGroupPermissionsRequest {
                permissions: vec![permission.into()],
            }),
        )
        .await
        .expect("set permissions");
        groups::add_group_member(
            axum::extract::Path(id),
            axum::Json(groups::AddGroupMemberRequest {
                address: address.into(),
            }),
        )
        .await
        .expect("add member");
    }

    /// Sign in through `POST /api/auth/login`; the session token.
    pub async fn login(&self, address: &str, password: &str) -> String {
        let res = self
            .http
            .post(format!("{}/api/auth/login", self.base))
            .json(&serde_json::json!({ "address": address, "password": password }))
            .send()
            .await
            .expect("login");
        assert_eq!(res.status(), 200, "login {address}");
        let body: serde_json::Value = res.json().await.expect("login body");
        body["token"].as_str().expect("token").to_string()
    }

    pub async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        bearer: &str,
        body: Option<serde_json::Value>,
    ) -> (u16, serde_json::Value) {
        let mut req = self
            .http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(bearer);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let res = req.send().await.expect("request");
        let status = res.status().as_u16();
        let text = res.text().await.expect("body");
        let value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        (status, value)
    }

    /// Open an MCP session as whoever `bearer` names.
    pub async fn mcp(&self, bearer: &str) -> Mcp<'_> {
        let mut mcp = Mcp {
            live: self,
            bearer: bearer.to_string(),
            session: None,
            next_id: 1,
        };
        mcp.rpc(
            "initialize",
            serde_json::json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "webapi-test", "version": "0" },
            }),
        )
        .await
        .expect("initialize");
        mcp.notify("notifications/initialized").await;
        mcp
    }
}

pub struct Mcp<'a> {
    live: &'a Live,
    bearer: String,
    session: Option<String>,
    next_id: u64,
}

impl Mcp<'_> {
    fn post(&self, body: serde_json::Value) -> reqwest::RequestBuilder {
        let mut req = self
            .live
            .http
            .post(format!("{}/mcp", self.live.base))
            .bearer_auth(&self.bearer)
            .header("accept", "application/json, text/event-stream")
            .json(&body);
        if let Some(s) = &self.session {
            req = req.header("mcp-session-id", s);
        }
        req
    }

    async fn notify(&self, method: &str) {
        let res = self
            .post(serde_json::json!({ "jsonrpc": "2.0", "method": method }))
            .send()
            .await
            .expect("notify");
        assert!(res.status().is_success(), "{method}: {}", res.status());
    }

    /// One JSON-RPC request; `Ok(result)` or `Err(error)`.
    async fn rpc(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, serde_json::Value> {
        let id = self.next_id;
        self.next_id += 1;
        let res = self
            .post(serde_json::json!({
                "jsonrpc": "2.0", "id": id, "method": method, "params": params,
            }))
            .send()
            .await
            .expect("mcp request");
        assert!(res.status().is_success(), "{method}: {}", res.status());
        if let Some(s) = res.headers().get("mcp-session-id") {
            self.session = Some(s.to_str().expect("session id").to_string());
        }
        let text = res.text().await.expect("mcp body");
        let msg = text
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .filter_map(|d| serde_json::from_str::<serde_json::Value>(d.trim()).ok())
            .find(|m| m["id"] == id)
            .unwrap_or_else(|| panic!("{method}: no response in {text:?}"));
        match msg.get("error") {
            Some(e) => Err(e.clone()),
            None => Ok(msg["result"].clone()),
        }
    }

    /// Call a tool. `Ok` carries the tool's JSON text parsed; `Err` the
    /// JSON-RPC error a failing tool answers with, or the text of a result
    /// marked `isError` (how invalid arguments come back since MCP moved
    /// input validation into the tool result).
    pub async fn tool(
        &mut self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, serde_json::Value> {
        let result = self
            .rpc(
                "tools/call",
                serde_json::json!({ "name": name, "arguments": arguments }),
            )
            .await?;
        let text = result["content"][0]["text"].as_str().unwrap_or("null");
        if result["isError"] == true {
            return Err(serde_json::json!({ "message": text }));
        }
        Ok(serde_json::from_str(text).expect("tool text is json"))
    }
}
