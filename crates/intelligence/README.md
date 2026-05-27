# mailrs-intelligence

[![Crates.io](https://img.shields.io/crates/v/mailrs-intelligence?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-intelligence)
[![docs.rs](https://img.shields.io/docsrs/mailrs-intelligence?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-intelligence)
[![License](https://img.shields.io/crates/l/mailrs-intelligence?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-intelligence?style=flat-square)](https://crates.io/crates/mailrs-intelligence)

LLM-powered email analysis primitives for Rust — pluggable provider, OpenAI-compatible reference impl, with no-LLM heuristics included.

Extracted from [mailrs] so any Rust project that needs to classify, summarize, or embed email content can lean on the same primitives without dragging in the entire mail server.

## Highlights

- **Pluggable backend** — `LlmProvider` trait keeps the choice of model visible at the call site, so small-core vs big-core decisions stay grep-auditable in your own code.
- **OpenAI-compatible reference impl** — `OpenAiCompatibleProvider` wraps `reqwest` and works against any service speaking the standard `{system, messages, temperature}` shape (self-hosted vLLM, llama.cpp servers, etc.).
- **Five primitives, one shape** — full email analysis (`analyze::analyze_email`), spam classification with optional cache (`spam::classify`), structured-data extraction from JSON-LD (`structured::extract_structured_data`), heuristic importance scoring (`importance::calculate_importance`), and embeddings via the provider's `embed` method.
- **No-LLM modules** — `importance` and `structured` are pure heuristics — they don't need a provider, network, or async runtime.
- **Optional Redis cache** — `RedisSpamCache` is included behind the default `redis-cache` feature, but `SpamCache` is a trait so you can plug in whatever store you have.

## Quick start

```rust
use std::sync::Arc;

use mailrs_intelligence::{
    OpenAiCompatibleProvider,
    analyze::{analyze_email, PROMPT_VERSION},
    importance::{calculate_importance, ImportanceSignals},
    LlmProvider,
};

# async fn run() -> Option<()> {
let provider: Arc<dyn LlmProvider> = Arc::new(OpenAiCompatibleProvider::new(
    "https://llm.example.com/complete".into(),
    Some("sk-…".into()),
    format!("qwen3.5-9b/{PROMPT_VERSION}"),
));

// LLM-powered full analysis
let analysis = analyze_email(
    provider.as_ref(),
    "boss@example.com",
    "Q3 review",
    "Please send your Q3 self-review by Friday.",
)
.await?;
println!("category={} requires_action={}", analysis.category, analysis.requires_action);

// No-LLM heuristic importance
let (level, score) = calculate_importance(&ImportanceSignals {
    is_mutual_contact: true,
    is_reply_to_my_email: true,
    has_action_items: analysis.requires_action,
    ..Default::default()
});
println!("importance={} score={:.2}", level.as_str(), score);
# Some(())
# }
```

## Feature flags

| Flag | Default | What it enables |
|------|---------|-----------------|
| `http`        | yes | `OpenAiCompatibleProvider` (reqwest + rustls). Disable if you supply your own `LlmProvider`. |
| `redis-cache` | yes | `RedisSpamCache` for `spam::classify`. Disable if you cache yourself or run without a cache. |

Disable both default features (`default-features = false`) if you're plugging in your own backends:

```toml
[dependencies]
mailrs-intelligence = { version = "1", default-features = false }
async-trait = "0.1"
```

## Why a trait

Production mail servers tend to mix cheap inference (per-message spam classification, hot path) with rarer expensive calls (deep structured extraction, big context). Letting analysis functions take `&dyn LlmProvider` keeps the choice of model **visible at the call site** — you can grep your own code for "which provider does this path hand into `analyze_email`?" without diving into config or environment variables. That visibility is the whole point of carving the trait out instead of shipping the concrete config struct.

<!-- AUDIT-FOOTER:BEGIN -->

## Stone audit (v3 cycle, 2026-05-25)

| Axis | Status |
|---|---|
| **doc** | ✅ clean (`cargo doc --no-deps -p mailrs-intelligence`) |
| **test** | line cov: 92.2% (`cargo llvm-cov -p mailrs-intelligence --summary-only`) |
| **bench** | ✅ 1 file(s) criterion + ✅ 2 gate(s) `perf_gate.rs` |
| **size** | release rlib: 1.4 MB |
| **fuzz** | ❌ none |
| **mem**  | dhat profile pending (v3.4 backlog) |

### Competitor comparisons

- Searched crates.io + competing impls: see PERFORMANCE.md or 'first-in-Rust' marker.

<!-- AUDIT-FOOTER:END -->

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

[mailrs]: https://github.com/goliajp/mailrs

## Performance

Criterion benches: `cargo bench -p mailrs-intelligence`. Per-bench medians + regression budgets are documented in [`BUDGETS.md`](BUDGETS.md) (this crate) and the workspace [`PERFORMANCE.md`](../../PERFORMANCE.md).
