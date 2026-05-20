//! Minimal example: build an OpenAI-compatible provider, analyze one email.
//!
//! Run against a self-hosted LLM endpoint:
//!
//! ```bash
//! LLM_URL=https://llm.example.com/complete \
//! LLM_API_KEY=sk-... \
//! cargo run --example basic
//! ```

use std::sync::Arc;

use mailrs_intelligence::analyze::{PROMPT_VERSION, analyze_email};
use mailrs_intelligence::provider::LlmProvider;
use mailrs_intelligence::{OpenAiCompatibleProvider, importance, spam};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let url = std::env::var("LLM_URL").expect("LLM_URL required");
    let api_key = std::env::var("LLM_API_KEY").ok();

    let provider: Arc<dyn LlmProvider> = Arc::new(OpenAiCompatibleProvider::new(
        url,
        api_key,
        format!("qwen3.5-9b/{PROMPT_VERSION}"),
    ));

    // 1) full LLM analysis
    let analysis = analyze_email(
        provider.as_ref(),
        "boss@example.com",
        "Q3 review",
        "Please send your Q3 self-review by end of week.",
    )
    .await;

    match analysis {
        Some(a) => println!(
            "analysis: category={} risk={} requires_action={} summary={:?}",
            a.category, a.risk_score, a.requires_action, a.summary
        ),
        None => eprintln!("analysis failed (check provider URL / network)"),
    }

    // 2) spam classification — no cache for this example
    let spam_result = spam::classify(
        provider.as_ref(),
        None,
        "promo@unknown.example",
        "You won! Click here!!!",
        "Congratulations, claim your prize…",
    )
    .await;
    if let Some(r) = spam_result {
        println!("spam: score={:.1} reason={}", r.score, r.reason);
    }

    // 3) heuristic importance — no LLM call
    let (level, score) = importance::calculate_importance(&importance::ImportanceSignals {
        is_mutual_contact: true,
        is_direct_recipient: true,
        is_reply_to_my_email: true,
        ..Default::default()
    });
    println!("importance: level={} score={:.2}", level.as_str(), score);
}
