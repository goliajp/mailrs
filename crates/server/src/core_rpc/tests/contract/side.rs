//! Side state each core serves from network kevy (drafts, signatures,
//! templates, webhooks).

use mailrs_core_api::method::admin::{
    CreateWebhookRequest, SaveDraftRequest, SaveSignatureRequest, SaveTemplateRequest,
};

use super::ALICE;
use super::harness::{Cores, Diffs};

/// Ids and times each store mints for itself.
const MINTED: &[&str] = &["id", "created_at", "updated_at", "signing_secret"];

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    let (fc, pg) = (&c.fastcore, &c.pg_core);

    let req = SaveDraftRequest {
        id: None,
        to: "bob@partner.test".into(),
        cc: String::new(),
        bcc: String::new(),
        subject: "Draft".into(),
        body: "text".into(),
        reply_to_thread_id: None,
    };
    let saved = d.both(
        "save_draft",
        fc.save_draft(ALICE, &req).await,
        pg.save_draft(ALICE, &req).await,
        MINTED,
    );
    d.both(
        "list_drafts",
        fc.list_drafts(ALICE).await,
        pg.list_drafts(ALICE).await,
        MINTED,
    );
    if let Some((a, b)) = saved {
        d.both(
            "delete_draft",
            fc.delete_draft(ALICE, a.id).await,
            pg.delete_draft(ALICE, b.id).await,
            &[],
        );
    }
    d.both(
        "list_drafts after delete",
        fc.list_drafts(ALICE).await,
        pg.list_drafts(ALICE).await,
        MINTED,
    );

    let req = SaveSignatureRequest {
        name: "work".into(),
        html: "<p>A</p>".into(),
        text_content: "A".into(),
        is_default: true,
    };
    let saved = d.both(
        "save_signature",
        fc.save_signature(ALICE, &req).await,
        pg.save_signature(ALICE, &req).await,
        MINTED,
    );
    if let Some((a, b)) = saved {
        d.both(
            "delete_signature",
            fc.delete_signature(ALICE, a.id).await,
            pg.delete_signature(ALICE, b.id).await,
            &[],
        );
    }

    let req = SaveTemplateRequest {
        name: "thanks".into(),
        subject: "Thanks".into(),
        html_body: String::new(),
        text_body: "Thank you".into(),
        category: "general".into(),
        is_default: false,
    };
    let saved = d.both(
        "save_template",
        fc.save_template(ALICE, &req).await,
        pg.save_template(ALICE, &req).await,
        MINTED,
    );
    d.both(
        "list_templates",
        fc.list_templates(ALICE).await,
        pg.list_templates(ALICE).await,
        MINTED,
    );
    if let Some((a, b)) = saved {
        d.both(
            "delete_template",
            fc.delete_template(ALICE, a.id).await,
            pg.delete_template(ALICE, b.id).await,
            &[],
        );
    }

    let req = CreateWebhookRequest {
        account_address: ALICE.into(),
        url: "https://hooks.partner.test/in".into(),
        event_type: "new_message".into(),
        filter_sender: None,
        filter_thread_id: None,
    };
    let made = d.both(
        "create_webhook",
        fc.create_webhook(&req).await,
        pg.create_webhook(&req).await,
        MINTED,
    );
    d.both(
        "list_webhooks",
        fc.list_webhooks(ALICE).await,
        pg.list_webhooks(ALICE).await,
        MINTED,
    );
    if let Some((a, b)) = made {
        d.both(
            "delete_webhook",
            fc.delete_webhook(a.id).await,
            pg.delete_webhook(b.id).await,
            &[],
        );
    }
}
