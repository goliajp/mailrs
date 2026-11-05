//! What is parsed out of an inbound message before the rules and the
//! pipeline run, read in the parse worker: what the attachments hold
//! and the Bayes tokens, in one request.

use mailrs_inbound::ReceiveContext;
use mailrs_parse_worker::{AttachmentsRead, Limit, Outcome, Want};

/// Read `ctx.message`, leave the tokens in `ctx.bayes_tokens` and return
/// what the attachments hold. When the worker fails on this message the
/// answer says so (a fact for the rules); when no worker is available it
/// says nothing. Either way the tokens stay empty, so Bayes adds nothing,
/// and the message goes on.
pub(super) async fn read(ctx: &mut ReceiveContext) -> AttachmentsRead {
    let raw = std::mem::take(&mut ctx.message);
    let want = Want {
        inside: true,
        tokens: Some(mailrs_triage::spam::READ.tokenizer()),
        ..Want::default()
    };
    let handle = tokio::task::spawn_blocking(move || {
        let outcome = mailrs_parse_worker::read_message(&raw, want, Limit::Inbound);
        (raw, outcome)
    });
    let (raw, outcome) = match handle.await {
        Ok(done) => done,
        Err(e) => std::panic::resume_unwind(e.into_panic()),
    };
    ctx.message = raw;
    let read = AttachmentsRead::of(&outcome);
    if let Outcome::Read(r) = outcome {
        ctx.bayes_tokens = r.tokens;
    }
    read
}
