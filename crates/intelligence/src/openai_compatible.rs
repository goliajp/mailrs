use async_trait::async_trait;

use crate::provider::LlmProvider;

/// Reference [`LlmProvider`] backed by an OpenAI-compatible HTTP endpoint.
///
/// Concretely this is what mailrs uses against its self-hosted qwen3.5-9b
/// server — but any service that accepts the same `POST {system, messages,
/// temperature}` shape works. The embedding endpoint is derived by
/// substituting `/complete` → `/embed` in the URL.
#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProvider {
    url: String,
    api_key: Option<String>,
    model_id: String,
    client: reqwest::Client,
}

impl OpenAiCompatibleProvider {
    /// Construct a provider pointing at the given completion URL.
    ///
    /// `model_id` is what consumers persist alongside analysis results to
    /// detect when re-analysis is required (typical format:
    /// `"qwen3.5-9b/v8"`).
    pub fn new(url: String, api_key: Option<String>, model_id: String) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self {
            url,
            api_key,
            model_id,
            client,
        }
    }

    /// Construct with a caller-supplied `reqwest::Client` (e.g. one with a
    /// shared connection pool, custom TLS config, or proxy settings).
    pub fn with_client(
        url: String,
        api_key: Option<String>,
        model_id: String,
        client: reqwest::Client,
    ) -> Self {
        Self {
            url,
            api_key,
            model_id,
            client,
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    async fn complete(
        &self,
        system: &str,
        user_message: &str,
        temperature: f32,
    ) -> Option<String> {
        let body = serde_json::json!({
            "system": system,
            "messages": [{"role": "user", "content": user_message}],
            "temperature": temperature
        });

        // no `format` param — uses stream mode on server side (no timeout as long as tokens flow)
        // 900s safety timeout covers entire send+read cycle
        for attempt in 0..3u32 {
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(900),
                async {
                    let mut req = self.client.post(&self.url).json(&body);
                    if let Some(ref key) = self.api_key {
                        req = req
                            .header("Authorization", format!("Bearer {key}"))
                            .header("x-caller", "mailrs");
                    }
                    let response = req.send().await?;

                    if response.status().as_u16() == 429 {
                        return Ok::<Option<String>, reqwest::Error>(Some("__429__".into()));
                    }

                    if !response.status().is_success() {
                        let status = response.status();
                        let text = response.text().await.unwrap_or_default();
                        tracing::warn!(
                            event = "llm_http_error",
                            status = %status,
                            body = %&text[..text.len().min(200)]
                        );
                        return Ok(None);
                    }

                    let json: serde_json::Value = response.json().await?;
                    Ok(json["content"].as_str().map(|s| s.to_string()))
                },
            )
            .await;

            match result {
                Ok(Ok(Some(ref s))) if s == "__429__" => {
                    let wait = if attempt < 2 { 15 } else { 30 };
                    tracing::warn!(
                        event = "llm_rate_limited",
                        attempt = attempt + 1,
                        retry_in_s = wait
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                    continue;
                }
                Ok(Ok(content)) => return content,
                Ok(Err(e)) => {
                    tracing::warn!(event = "llm_request_error", error = %e);
                    return None;
                }
                Err(_) => {
                    tracing::warn!(event = "llm_request_timeout", timeout_s = 900);
                    return None;
                }
            }
        }

        tracing::warn!(event = "llm_rate_limited_giving_up");
        None
    }

    async fn embed(&self, text: &str) -> Option<Vec<f32>> {
        let embed_url = self.url.replace("/complete", "/embed");
        let body = serde_json::json!({ "input": text });

        for attempt in 0..2u32 {
            let mut req = self.client.post(&embed_url).json(&body);
            if let Some(ref key) = self.api_key {
                req = req
                    .header("Authorization", format!("Bearer {key}"))
                    .header("x-caller", "mailrs");
            }
            let response = match tokio::time::timeout(
                std::time::Duration::from_secs(10),
                req.send(),
            )
            .await
            {
                Ok(Ok(resp)) => resp,
                Ok(Err(e)) => {
                    tracing::warn!(event = "embedding_request_error", error = %e);
                    return None;
                }
                Err(_) => {
                    tracing::warn!(event = "embedding_request_timeout", timeout_s = 10);
                    return None;
                }
            };

            if response.status().as_u16() == 429 {
                let wait = if attempt < 2 { 15 } else { 30 };
                tracing::warn!(
                    event = "embedding_rate_limited",
                    attempt = attempt + 1,
                    retry_in_s = wait
                );
                tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                continue;
            }

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                tracing::warn!(
                    event = "embedding_http_error",
                    status = %status,
                    body = %&text[..text.len().min(200)]
                );
                return None;
            }

            let json: serde_json::Value = match response.json().await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(event = "embedding_parse_error", error = %e);
                    return None;
                }
            };

            let values = json["embeddings"]
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_array())?;

            let embedding: Vec<f32> = values
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();

            let dims = json["dimensions"].as_u64().unwrap_or(1024) as usize;
            return if embedding.len() == dims {
                Some(embedding)
            } else {
                tracing::warn!(
                    event = "embedding_bad_dim",
                    got = embedding.len(),
                    expected = dims
                );
                None
            };
        }

        tracing::warn!(event = "embedding_giving_up");
        None
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }
}
