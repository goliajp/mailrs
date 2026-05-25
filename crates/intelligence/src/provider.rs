use async_trait::async_trait;

/// Pluggable LLM backend.
///
/// All analysis primitives in this crate (`analyze_email`, `classify_spam`,
/// `generate_embedding`, …) take `&dyn LlmProvider` so the caller is the one
/// who chooses *which* model handles *which* request — making small-core vs
/// big-core decisions explicit and grep-auditable in the consumer code.
///
/// A reference implementation backed by an OpenAI-compatible HTTP endpoint
/// is provided as [`crate::OpenAiCompatibleProvider`] under the default
/// `http` feature. To plug in another backend, implement this trait yourself
/// and disable the `http` feature.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Run a chat completion against the model.
    ///
    /// Returns `None` on transport error, non-success HTTP status, or any
    /// retry-exhaustion the implementation defines. Errors should already
    /// have been logged via `tracing` by the provider.
    async fn complete(&self, system: &str, user_message: &str, temperature: f32) -> Option<String>;

    /// Generate an embedding vector for `text`.
    ///
    /// Providers that don't expose an embedding endpoint should return
    /// `None`. Callers that always need embeddings should pair such a
    /// provider with an embedding-capable one rather than encoding the
    /// constraint into the trait.
    async fn embed(&self, text: &str) -> Option<Vec<f32>>;

    /// Stable identifier for the model + prompt revision in use.
    ///
    /// This is what consumers persist alongside a stored analysis result
    /// to know whether re-analysis is required after a prompt or model
    /// change. Typical format: `"qwen3.5-9b/v8"`.
    fn model_id(&self) -> &str;
}
