//! Token-based APNs, the provider side.
//!
//! Apple's contract, in the three parts that matter here:
//!
//! - Every request carries a **provider token**: an ES256 JWT whose `kid`
//!   is the `.p8` key's id and whose `iss` is the *team* id. The key is
//!   team-scoped — one key signs for every app the team owns, and a key
//!   from another team is refused no matter how valid its signature.
//! - Tokens must be **reused**: Apple rejects providers that mint one per
//!   request (`TooManyProviderTokenUpdates`), and refuses tokens older
//!   than an hour. This client refreshes at [`JWT_REFRESH_SECS`].
//! - The transport is **HTTP/2 only**. `api.push.apple.com` closes
//!   HTTP/1.1 connections, which is why reqwest is built with `http2`.
//!
//! The endpoint is a constructor argument rather than a constant so the
//! sandbox gateway and a local stub are the same code path as production.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};

/// Apple's production gateway.
pub const PRODUCTION_ENDPOINT: &str = "https://api.push.apple.com";
/// The sandbox gateway — where tokens from Xcode debug builds live. A
/// sandbox token pushed at production answers `BadDeviceToken`, which is
/// indistinguishable from a rotten token; getting the environment wrong
/// therefore looks exactly like every device unregistering at once.
pub const SANDBOX_ENDPOINT: &str = "https://api.sandbox.push.apple.com";

/// Refresh under Apple's one-hour ceiling, with margin for clock skew and
/// for the request sitting in a queue.
const JWT_REFRESH_SECS: u64 = 45 * 60;

#[derive(Debug, thiserror::Error)]
pub enum ApnsError {
    #[error("the key is not a PEM-wrapped PKCS#8 EC key: {0}")]
    Key(String),
    #[error("signing failed")]
    Sign,
    #[error("transport: {0}")]
    Transport(String),
}

/// What became of one push.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Sent,
    /// The device token is dead — uninstalled, or a sandbox token sent to
    /// production. The caller should delete it: Apple throttles providers
    /// that keep pushing to tokens it has already refused.
    Unregistered,
    /// Refused for a reason that is not the token's fault.
    Failed {
        status: u16,
        reason: String,
    },
}

pub struct ApnsClient {
    key: EcdsaKeyPair,
    key_id: String,
    team_id: String,
    topic: String,
    endpoint: String,
    http: reqwest::Client,
    cached_jwt: Mutex<Option<(String, u64)>>,
    rng: SystemRandom,
}

impl ApnsClient {
    /// `key_pem` is the `.p8` exactly as Apple hands it out; `topic` is
    /// the app's bundle id.
    pub fn new(
        key_pem: &str,
        key_id: impl Into<String>,
        team_id: impl Into<String>,
        topic: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Result<Self, ApnsError> {
        let der = pem_to_der(key_pem)?;
        let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &der)
            .map_err(|e| ApnsError::Key(e.to_string()))?;
        let http = reqwest::Client::builder()
            .build()
            .map_err(|e| ApnsError::Transport(e.to_string()))?;
        Ok(Self {
            key,
            key_id: key_id.into(),
            team_id: team_id.into(),
            topic: topic.into(),
            endpoint: endpoint.into(),
            http,
            cached_jwt: Mutex::new(None),
            rng: SystemRandom::new(),
        })
    }

    /// One alert to one device.
    pub async fn send_alert(
        &self,
        device_token: &str,
        title: &str,
        body: &str,
    ) -> Result<Outcome, ApnsError> {
        let payload = serde_json::json!({
            "aps": { "alert": { "title": title, "body": body } }
        });
        let jwt = self.provider_token()?;
        let url = format!("{}/3/device/{}", self.endpoint, device_token);
        let response = self
            .http
            .post(&url)
            .header("authorization", format!("bearer {jwt}"))
            .header("apns-topic", &self.topic)
            .header("apns-push-type", "alert")
            .header("apns-priority", "10")
            .json_body(&payload)
            .send()
            .await
            .map_err(|e| ApnsError::Transport(e.to_string()))?;

        let status = response.status().as_u16();
        if (200..300).contains(&status) {
            return Ok(Outcome::Sent);
        }
        let reason = response
            .text()
            .await
            .ok()
            .and_then(|text| {
                serde_json::from_str::<serde_json::Value>(&text)
                    .ok()
                    .and_then(|v| v["reason"].as_str().map(String::from))
            })
            .unwrap_or_default();
        // 410 is the definitive "gone"; `BadDeviceToken` on 400 is the
        // same fact told differently (most often an environment
        // mismatch). Both mean "stop sending to this token".
        if status == 410 || reason == "Unregistered" || reason == "BadDeviceToken" {
            return Ok(Outcome::Unregistered);
        }
        Ok(Outcome::Failed { status, reason })
    }

    /// The cached provider JWT, minting a fresh one past the refresh age.
    fn provider_token(&self) -> Result<String, ApnsError> {
        let now = unix_now();
        let mut cached = self.cached_jwt.lock().expect("jwt lock");
        if let Some((jwt, issued_at)) = cached.as_ref()
            && now.saturating_sub(*issued_at) < JWT_REFRESH_SECS
        {
            return Ok(jwt.clone());
        }
        let jwt = self.mint_jwt(now)?;
        *cached = Some((jwt.clone(), now));
        Ok(jwt)
    }

    fn mint_jwt(&self, issued_at: u64) -> Result<String, ApnsError> {
        let header = serde_json::json!({ "alg": "ES256", "kid": self.key_id });
        let claims = serde_json::json!({ "iss": self.team_id, "iat": issued_at });
        let signing_input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        // FIXED, not ASN.1: JWS ES256 wants the raw 64-byte r||s form.
        let signature = self
            .key
            .sign(&self.rng, signing_input.as_bytes())
            .map_err(|_| ApnsError::Sign)?;
        Ok(format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signature.as_ref())
        ))
    }
}

/// A tiny extension so the request body is JSON without reqwest's `json`
/// feature — one serialize call does not need the feature's dependency on
/// serde integration across the crate boundary.
trait JsonBody {
    fn json_body(self, value: &serde_json::Value) -> Self;
}

impl JsonBody for reqwest::RequestBuilder {
    fn json_body(self, value: &serde_json::Value) -> Self {
        self.header("content-type", "application/json")
            .body(value.to_string())
    }
}

/// Strip the PEM armour off a `.p8` and decode the PKCS#8 DER inside.
fn pem_to_der(pem: &str) -> Result<Vec<u8>, ApnsError> {
    let inner: String = pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    STANDARD
        .decode(inner.trim())
        .map_err(|e| ApnsError::Key(e.to_string()))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lc_rs::signature::{ECDSA_P256_SHA256_FIXED, UnparsedPublicKey};

    /// A fresh P-256 key in the same PEM shape Apple ships, plus its
    /// public half so tests can verify what the client signs.
    fn generated_key() -> (String, Vec<u8>) {
        let rng = SystemRandom::new();
        let document =
            EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).expect("generate");
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, document.as_ref())
            .expect("reparse");
        use aws_lc_rs::signature::KeyPair as _;
        let public = pair.public_key().as_ref().to_vec();
        let pem = format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
            STANDARD.encode(document.as_ref())
        );
        (pem, public)
    }

    fn client(pem: &str, endpoint: &str) -> ApnsClient {
        ApnsClient::new(pem, "KEYID12345", "TEAMID1234", "jp.golia.mailrs", endpoint)
            .expect("client")
    }

    /// The JWT is what Apple validates; this verifies it the way Apple
    /// would — signature over `header.claims` with the public key, and
    /// the ids in the fields Apple reads them from.
    #[test]
    fn provider_token_is_a_valid_es256_jwt() {
        let (pem, public) = generated_key();
        let c = client(&pem, "http://unused");
        let jwt = c.provider_token().expect("jwt");

        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3, "not a JWT: {jwt}");

        let header: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(header["alg"], "ES256");
        assert_eq!(header["kid"], "KEYID12345");

        let claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(claims["iss"], "TEAMID1234");
        assert!(claims["iat"].as_u64().unwrap() > 0);

        let signature = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
        let message = format!("{}.{}", parts[0], parts[1]);
        UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, &public)
            .verify(message.as_bytes(), &signature)
            .expect("signature must verify with the public half");
    }

    /// Apple refuses providers that mint a token per request, so the same
    /// token must come back while it is fresh.
    #[test]
    fn provider_token_is_cached() {
        let (pem, _) = generated_key();
        let c = client(&pem, "http://unused");
        assert_eq!(c.provider_token().unwrap(), c.provider_token().unwrap());
    }

    #[test]
    fn rejects_garbage_keys() {
        assert!(ApnsClient::new("not a key", "k", "t", "topic", "http://x").is_err());
        let wrapped = "-----BEGIN PRIVATE KEY-----\naGVsbG8=\n-----END PRIVATE KEY-----";
        assert!(ApnsClient::new(wrapped, "k", "t", "topic", "http://x").is_err());
    }

    /// One stub request cycle: the headers Apple requires are present,
    /// the path carries the device token, and a 410 comes back as
    /// `Unregistered` — the outcome callers prune on.
    #[tokio::test]
    async fn sends_and_maps_410_to_unregistered() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = sock.read(&mut buf).await.unwrap();
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let response = "HTTP/1.1 410 Gone\r\ncontent-type: application/json\r\ncontent-length: 25\r\n\r\n{\"reason\":\"Unregistered\"}";
            sock.write_all(response.as_bytes()).await.unwrap();
            request
        });

        let (pem, _) = generated_key();
        let c = client(&pem, &format!("http://{addr}"));
        let outcome = c.send_alert("dead-token", "Title", "Body").await.unwrap();
        assert_eq!(outcome, Outcome::Unregistered);

        let request = server.await.unwrap();
        assert!(
            request.starts_with("POST /3/device/dead-token "),
            "{request}"
        );
        assert!(request.contains("authorization: bearer "), "{request}");
        assert!(request.contains("apns-topic: jp.golia.mailrs"), "{request}");
        assert!(request.contains("apns-push-type: alert"), "{request}");
    }
}
