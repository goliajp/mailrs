//! Verifying an external provider's `id_token` before trusting a word of it:
//! the signature against the provider's published keys, the issuer, the
//! audience (this deployment's client id), the expiry and the nonce this
//! deployment sent when the sign-in started.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use mailrs_oauth_client::Provider;

/// How long a provider's keys are used before they are fetched again. A
/// token naming a key we do not hold fetches at once, so a rotation is
/// picked up on its first use.
const KEYS_FRESH: Duration = Duration::from_secs(3600);

/// Signatures a provider's id_token may carry. Never `none`, never a
/// shared-secret algorithm.
const ALGORITHMS: &[Algorithm] = &[Algorithm::RS256, Algorithm::ES256, Algorithm::PS256];

static KEYS: LazyLock<Mutex<HashMap<String, (Instant, JwkSet)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The verified claims.
pub async fn verify(
    provider: &Provider,
    token: &str,
    nonce: &str,
) -> Result<serde_json::Value, String> {
    let kid = jsonwebtoken::decode_header(token)
        .map_err(|e| format!("id_token header: {e}"))?
        .kid
        .unwrap_or_default();
    let cached = KEYS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&provider.issuer)
        .filter(|(at, keys)| at.elapsed() < KEYS_FRESH && keys.find(&kid).is_some())
        .map(|(_, keys)| keys.clone());
    let keys = match cached {
        Some(keys) => keys,
        None => {
            let keys = fetch_keys(&provider.issuer).await?;
            KEYS.lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(provider.issuer.clone(), (Instant::now(), keys.clone()));
            keys
        }
    };
    verify_with(&keys, provider, token, nonce)
}

/// [`verify`] against keys already in hand.
pub fn verify_with(
    keys: &JwkSet,
    provider: &Provider,
    token: &str,
    nonce: &str,
) -> Result<serde_json::Value, String> {
    let header = jsonwebtoken::decode_header(token).map_err(|e| format!("id_token header: {e}"))?;
    if !ALGORITHMS.contains(&header.alg) {
        return Err(format!("id_token signed with {:?}", header.alg));
    }
    let kid = header.kid.as_deref().unwrap_or_default();
    let jwk = keys
        .find(kid)
        .ok_or_else(|| format!("no published key {kid:?}"))?;
    let key = DecodingKey::from_jwk(jwk).map_err(|e| format!("key {kid:?}: {e}"))?;
    let mut validation = Validation::new(header.alg);
    validation.set_issuer(&[&provider.issuer]);
    validation.set_audience(&[&provider.client_id]);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    let claims = jsonwebtoken::decode::<serde_json::Value>(token, &key, &validation)
        .map_err(|e| format!("id_token: {e}"))?
        .claims;
    match claims.get("nonce").and_then(|n| n.as_str()) {
        Some(n) if !nonce.is_empty() && n == nonce => Ok(claims),
        _ => Err("id_token nonce is not the one this sign-in sent".into()),
    }
}

async fn fetch_keys(issuer: &str) -> Result<JwkSet, String> {
    let egress = mailrs_egress::Egress::new("mailrs-oidc/1.0", Duration::from_secs(10));
    let discovery = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let config: serde_json::Value = fetch_json(&egress, &discovery).await?;
    let jwks_uri = config
        .get("jwks_uri")
        .and_then(|u| u.as_str())
        .ok_or("discovery document has no jwks_uri")?
        .to_string();
    let keys = fetch_json(&egress, &jwks_uri).await?;
    serde_json::from_value(keys).map_err(|e| format!("jwks: {e}"))
}

async fn fetch_json(
    egress: &mailrs_egress::Egress,
    url: &str,
) -> Result<serde_json::Value, String> {
    let resp = egress
        .send(url, 2, |c, u| c.get(u))
        .await
        .map_err(|e| format!("{url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("{url}: HTTP {}", resp.status().as_u16()));
    }
    let body = mailrs_egress::read_capped(resp, 1 << 20)
        .await
        .map_err(|e| format!("{url}: {e}"))?;
    serde_json::from_slice(&body).map_err(|e| format!("{url}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lc_rs::rand::SystemRandom;
    use aws_lc_rs::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
    use base64::Engine as _;
    use jsonwebtoken::{EncodingKey, Header};

    fn provider() -> Provider {
        Provider {
            key: "portal".into(),
            issuer: "https://idp.example.com".into(),
            authorize_url: String::new(),
            token_url: String::new(),
            userinfo_url: None,
            scopes: Vec::new(),
            client_id: "mailrs-client".into(),
            redirect_uri: String::new(),
            source: mailrs_oauth_client::IdentitySource::IdToken,
            require_verified_email: true,
        }
    }

    /// A key made for the test, its public half as a JWK set, and a
    /// signer of claims with it.
    fn keypair(kid: &str) -> (JwkSet, impl Fn(serde_json::Value) -> String) {
        let pkcs8 =
            EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
                .unwrap();
        let pair =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref()).unwrap();
        let point = pair.public_key().as_ref().to_vec();
        let b64 = |b: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b);
        let set: JwkSet = serde_json::from_value(serde_json::json!({ "keys": [{
            "kty": "EC", "crv": "P-256", "alg": "ES256", "use": "sig", "kid": kid,
            "x": b64(&point[1..33]), "y": b64(&point[33..65]),
        }]}))
        .unwrap();
        let der = pkcs8.as_ref().to_vec();
        let kid = kid.to_string();
        let sign = move |claims: serde_json::Value| {
            let mut header = Header::new(Algorithm::ES256);
            header.kid = Some(kid.clone());
            jsonwebtoken::encode(&header, &claims, &EncodingKey::from_ec_der(&der)).unwrap()
        };
        (set, sign)
    }

    fn claims(nonce: &str) -> serde_json::Value {
        serde_json::json!({
            "iss": "https://idp.example.com",
            "aud": "mailrs-client",
            "sub": "u1",
            "exp": mailrs_auth_guard::login::unix_now() + 300,
            "nonce": nonce,
        })
    }

    #[test]
    fn a_token_signed_by_the_provider_for_this_client_passes() {
        let (keys, sign) = keypair("k1");
        let got = verify_with(&keys, &provider(), &sign(claims("n1")), "n1").unwrap();
        assert_eq!(got["sub"], "u1");
    }

    #[test]
    fn anything_else_is_refused() {
        let (keys, sign) = keypair("k1");
        let p = provider();
        let with = |f: &dyn Fn(&mut serde_json::Value)| {
            let mut c = claims("n1");
            f(&mut c);
            sign(c)
        };
        assert!(
            verify_with(&keys, &p, &sign(claims("n1")), "other").is_err(),
            "nonce"
        );
        assert!(
            verify_with(&keys, &p, &sign(claims("n1")), "").is_err(),
            "no nonce kept"
        );
        assert!(
            verify_with(
                &keys,
                &p,
                &with(&|c| c["aud"] = "someone-else".into()),
                "n1"
            )
            .is_err(),
            "aud"
        );
        assert!(
            verify_with(
                &keys,
                &p,
                &with(&|c| c["iss"] = "https://evil.example".into()),
                "n1"
            )
            .is_err(),
            "iss"
        );
        assert!(
            verify_with(&keys, &p, &with(&|c| c["exp"] = 1_000_000.into()), "n1").is_err(),
            "exp"
        );
        // signed by a key the provider never published
        let (_, forge) = keypair("k1");
        assert!(
            verify_with(&keys, &p, &forge(claims("n1")), "n1").is_err(),
            "signature"
        );
        // the unsigned form
        let b64 = |s: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s);
        let none = format!(
            "{}.{}.",
            b64(r#"{"alg":"none","kid":"k1"}"#),
            b64(&claims("n1").to_string())
        );
        assert!(verify_with(&keys, &p, &none, "n1").is_err(), "alg none");
    }
}
