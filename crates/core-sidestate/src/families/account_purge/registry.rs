//! Every store and key family that holds an account's data, in one place.
//!
//! The purge removes exactly what is listed here, and
//! `scripts/check-shared-keys.sh` reads this file: a network family it
//! checks, or a key built from a user's address anywhere in `crates/`, that
//! no entry below names (as owned, or as kept with a reason) fails it. So a
//! new per-user family cannot ship without its purge.
//!
//! Each entry's `name` is the key with `{user}` where the address goes and
//! `*` for any other part. The check matches on it; the purge reports by it.
//! Several key builders here are spelled where nothing else defines them;
//! the check exempts this file from the one-spelling rule for that reason.
//!
//! Order matters within a store: an entry that finds entries through an
//! index runs before the entry that removes the index.

use super::shape::{Shape, glob_escape};
use super::{custom, last_reader, rejudge};
use crate::families::{
    burner_released, calendar_events, calendar_feeds, callback_numbers, contact_cards,
    contact_sent, contacts, devices, email_groups, forwarding, local_forward, masked_email, notify,
    outbound::pause, payment_accounts, permission_groups, prefs, recall_notices, rules,
    saved_searches, sectioned_inbox, send, sender_lists, sender_rules, shared_inbox, submitted,
    swipe, vacation,
};
use crate::keys::{credentials, push, quota, reset, session};

pub use super::sql_tables::{SQL_CASCADED, SQL_TABLES};

/// Which store a family lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Store {
    /// The network kevy every process shares.
    Network,
    /// fastcore's embedded kevy (the SQL core's rows are [`SQL_TABLES`]).
    Mail,
}

#[derive(Debug, Clone, Copy)]
pub struct Owned {
    pub name: &'static str,
    pub store: Store,
    pub shape: Shape,
}

const fn net(name: &'static str, shape: Shape) -> Owned {
    Owned {
        name,
        store: Store::Network,
        shape,
    }
}

const fn mail(name: &'static str, shape: Shape) -> Owned {
    Owned {
        name,
        store: Store::Mail,
        shape,
    }
}

fn under(prefix: &str, a: &str, sep: &str) -> Vec<String> {
    vec![format!("{prefix}{}{sep}*", glob_escape(a))]
}

/// What an account owns, store by store.
#[rustfmt::skip]
pub const OWNED: &[Owned] = &[
    // credentials and sign-in
    net("session:by_addr:{user}", Shape::Custom(custom::sessions)),
    net("session:by_addr:{user}", Shape::Key(session::by_addr_key)),
    net("agent:key:secret:*", Shape::Custom(custom::agent_key_index)),
    net("agent:keys:{user}", Shape::Key(credentials::agent_keys_key)),
    net("agent:keys:last_used:{user}", Shape::Key(credentials::agent_keys_last_used_key)),
    net("agent:keys:counter:{user}", Shape::Key(credentials::agent_keys_counter_key)),
    net("totp:{user}", Shape::Key(credentials::totp_key)),
    net("pwreset_by_addr:{user}", Shape::Custom(custom::password_reset_token)),
    net("pwreset_by_addr:{user}", Shape::Key(credentials::pwreset_by_addr_key)),
    net("pwreset:ratelimit:{user}", Shape::Key(credentials::pwreset_ratelimit_key)),
    net("reset:code:{user}", Shape::Key(reset::code_key)),
    net("reset:failures:{user}", Shape::Key(reset::failures_key)),
    net("app_pw:*{user}", Shape::Globs(mailrs_auth_guard::app_passwords::account_key_globs)),
    net("ag:*{user}*", Shape::Globs(mailrs_auth_guard::login::account_key_globs)),
    net("auth:events:seen:*:{user}:*", Shape::Globs(mailrs_auth_guard::events::seen_globs)),
    net("auth:alerts:once:*:{user}", Shape::Globs(|a| vec![format!("auth:alerts:once:*:{}", glob_escape(a))])),
    net("oidc:links:{user}", Shape::Custom(custom::identity_links)),
    net("oidc:(token|refresh|code):*", Shape::Custom(custom::oidc_grants)),
    // mail handling the user set up
    net("sieve:{user}", Shape::Key(crate::sieve_key)),
    net("rules:{user}", Shape::Key(rules::key)),
    net("rules:{user}:*", Shape::Globs(|a| under("rules:", &a.to_lowercase(), ":"))),
    net("forwarding:{user}", Shape::Key(forwarding::key)),
    net("mailrs:forward:local:{user}", Shape::Key(local_forward::key)),
    net("mailrs:forward:local:*", Shape::Members { keys: || local_forward::key("*"), sep: None }),
    net("vacation:{user}", Shape::Key(vacation::key)),
    net("swipe:{user}", Shape::Key(swipe::key)),
    net("sectioned_inbox:{user}", Shape::Key(sectioned_inbox::key)),
    net("mailrs:vacation:{user}:*", Shape::Globs(|a| under("mailrs:vacation:", a, ":"))),
    net("spam:{user}:whitelist", Shape::Key(sender_lists::whitelist_key)),
    net("mailrs:burnerreleased:{user}", Shape::Key(burner_released::key)),
    net("spam:{user}:blacklist", Shape::Key(sender_lists::blacklist_key)),
    net("sender_rules:{user}", Shape::Key(sender_rules::rules_key)),
    net("sender_rules:retaining", Shape::Members { keys: || sender_rules::RETAINING_KEY.into(), sep: None }),
    // the SQL core's image prefetch queues
    net("mailrs:image-prefetch:pending", Shape::Members { keys: || "mailrs:image-prefetch:pending".into(), sep: Some("\t") }),
    net("mailrs:image-prefetch:asked", Shape::Members { keys: || "mailrs:image-prefetch:asked".into(), sep: Some("\t") }),
    net("corrections:{user}", Shape::Key(sender_rules::corrections_key)),
    net("saved_searches:{user}", Shape::Key(saved_searches::key)),
    net("saved_searches:{user}:*", Shape::Globs(|a| under("saved_searches:", &a.to_lowercase(), ":"))),
    net("spam_feedback:{user}", Shape::Key(|a| format!("spam_feedback:{a}"))),
    // what the user wrote and keeps
    net("drafts:{user}", Shape::Key(prefs::drafts_key)),
    net("drafts:{user}:counter", Shape::Key(prefs::drafts_counter_key)),
    net("signatures:{user}", Shape::Key(prefs::signatures_key)),
    net("signatures:{user}:counter", Shape::Key(prefs::signatures_counter_key)),
    net("templates:{user}", Shape::Key(prefs::templates_key)),
    net("templates:{user}:counter", Shape::Key(prefs::templates_counter_key)),
    net("mailrs:send:{user}:*", Shape::Globs(|a| vec![send::send_key(&glob_escape(a), "*")])),
    net("mailrs:submitted:{user}", Shape::Key(submitted::key)),
    net("mailrs:recalled:{user}", Shape::Key(recall_notices::key)),
    net("mailrs:imap-import:progress:{user}", Shape::Key(|a| format!("mailrs:imap-import:progress:{a}"))),
    net("mailrs:imap-import:started:{user}:*", Shape::Globs(|a| under("mailrs:imap-import:started:", a, ":"))),
    net("encryption_keys:{user}", Shape::Key(|a| format!("encryption_keys:{a}"))),
    net("pgp_keys:{user}", Shape::Key(|a| format!("pgp_keys:{a}"))),
    net("inline:*", Shape::Custom(custom::inline_uploads)),
    net("upload store {user} (files on disk)", Shape::Custom(custom::upload_dir)),
    net("inline-uploaded:{user}", Shape::Key(|a| format!("inline-uploaded:{}", a.to_lowercase()))),
    net("mailrs:jmap:snapshots:{user}", Shape::Key(|a| format!("mailrs:jmap:snapshots:{a}"))),
    net("mailrs:jmap:snapshot:{user}:*", Shape::Globs(|a| under("mailrs:jmap:snapshot:", a, ":"))),
    net("admin:account:{user}:overrides", Shape::Key(|a| format!("admin:account:{a}:overrides"))),
    // people, calendar
    net("mailrs:contact:{user}:*", Shape::Globs(|a| vec![contacts::contact_key(&glob_escape(a), "*")])),
    net("mailrs:contact_vips:{user}", Shape::Key(contacts::vips_key)),
    net("mailrs:callnum:{user}:*", Shape::Globs(|a| vec![callback_numbers::key(&glob_escape(a), "*")])),
    net("mailrs:payacct:{user}:*", Shape::Globs(|a| vec![payment_accounts::key(&glob_escape(a), "*")])),
    net("mailrs:contact_sent:{user}", Shape::Key(contact_sent::sent_to_key)),
    net("mailrs:user:{user}:contacts", Shape::Key(contact_cards::autocomplete_key)),
    net("mailrs:user:{user}:contacts:ts", Shape::Key(contact_cards::autocomplete_ts_key)),
    net("contact_cards:{user}", Shape::Key(contact_cards::cards_key)),
    net("calendar_events:{user}", Shape::Key(calendar_events::index_key)),
    net("calendar_event:{user}:*", Shape::Globs(|a| vec![calendar_events::event_key(&glob_escape(a), "*")])),
    net("calendar_event_names:{user}", Shape::Key(calendar_events::names::names_key)),
    net("calendar_event_hrefs:{user}", Shape::Key(calendar_events::names::hrefs_key)),
    net("calendar_feeds:{user}", Shape::Key(calendar_feeds::feeds_key)),
    net("rsvp:{user}:*", Shape::Globs(|a| under("rsvp:", a, ":"))),
    net("rsvp_counter:{user}:*", Shape::Globs(|a| under("rsvp_counter:", a, ":"))),
    // devices and notifications
    net("devices:{user}", Shape::Key(devices::key)),
    net("notify:{user}", Shape::Key(notify::prefs_key)),
    net("notify:{user}:*", Shape::Globs(|a| under("notify:", &a.to_lowercase(), ":"))),
    net("push:tokens:{user}", Shape::Key(push::tokens_key)),
    net("push:web:{user}", Shape::Key(push::web_subscriptions_key)),
    net("push:unifiedpush:{user}", Shape::Key(push::unified_push_key)),
    net("push:used:{user}", Shape::Key(push::used_key)),
    net("push:removed:{user}", Shape::Key(push::removed_key)),
    net("push:session:{user}", Shape::Key(push::session_link_key)),
    net("push:upwindow:{user}", Shape::Key(push::unified_push_window_key)),
    net("push:uptrailing:{user}", Shape::Key(push::unified_push_trailing_key)),
    net("push:stats:{user}:*", Shape::Globs(|a| under("push:stats:", &a.to_lowercase(), ":"))),
    // what the user's account is allowed and counted
    net("mailrs:quota:{user}:used_bytes", Shape::Key(quota::used_key)),
    net("mailrs:quota:{user}:limit_bytes", Shape::Key(quota::limit_key)),
    net("mailrs:outbound:withheld:{user}", Shape::Custom(crate::families::outbound::purge_withheld)),
    net("outbound:paused:{user}", Shape::Key(pause::paused_key)),
    net("outbound:held:{user}", Shape::Key(pause::held_key)),
    net("outbound:paused", Shape::Members { keys: || "outbound:paused".into(), sep: None }),
    net("outbound:rate:{user}:*", Shape::Globs(|a| under("outbound:rate:", a, ":"))),
    net("outbound:rate:day:{user}:*", Shape::Globs(|a| under("outbound:rate:day:", a, ":"))),
    net("outbound:bounce:day:{user}:*", Shape::Globs(|a| under("outbound:bounce:day:", a, ":"))),
    net("outbound:complaint:day:{user}:*", Shape::Globs(|a| under("outbound:complaint:day:", a, ":"))),
    net("outbound:fail:{user}:*", Shape::Globs(|a| under("outbound:fail:", a, ":"))),
    // integrations the user made
    net("admin:webhooks:{user}", Shape::Custom(crate::families::webhooks::purge_owner)),
    net("agent:webhooks:{user}", Shape::Key(|a| format!("agent:webhooks:{a}"))),
    net("agent:webhooks:counter:{user}", Shape::Key(|a| format!("agent:webhooks:counter:{a}"))),
    net("webhook:outbox:*", Shape::Custom(crate::families::webhook_outbox::purge_account)),
    net("mailrs:masked:user:{user}", Shape::Custom(custom::masks)),
    net("mailrs:masked:day:{user}:*", Shape::Globs(|a| vec![masked_email::day_glob(&glob_escape(a))])),
    net("mailrs:mock:owner:{user}", Shape::Custom(custom::mock_namespaces)),
    net("shared:claims:{user}", Shape::Key(shared_inbox::claims_key)),
    // where other records name the user
    net("admin:groups:*:members", Shape::Members { keys: || permission_groups::members_key("*"), sep: None }),
    net("admin:email-group:*:members", Shape::Members { keys: || email_groups::members_key("*"), sep: None }),
    net("mailrs:alias:v2:* (target)", Shape::Custom(custom::aliases_to)),
    net("admin:aliases (target)", Shape::Custom(custom::legacy_aliases_to)),
    net("mailrs:accountnames", Shape::Members { keys: || crate::families::account_names::KEY.into(), sep: None }),
    // fastcore's mail store; the conversations first, by the core's own
    // delete, so records other owners share stay with them
    mail("mailrs:threaduser:{user}:*", Shape::Globs(|a| under("mailrs:threaduser:", a, ":"))),
    mail("mailrs:usermsg:{user}:*", Shape::Globs(|a| under("mailrs:usermsg:", a, ":"))),
    mail("mailrs:usermsgs:{user}:*", Shape::Globs(|a| under("mailrs:usermsgs:", a, ":"))),
    mail("mailrs:usermsg-files:{user}:*", Shape::Globs(|a| under("mailrs:usermsg-files:", a, ":"))),
    mail("mailrs:message:by-message-id:{user}:*", Shape::Globs(|a| under("mailrs:message:by-message-id:", a, ":"))),
    mail("mailrs:message:by-maildir:{user}:*", Shape::Globs(|a| under("mailrs:message:by-maildir:", a, ":"))),
    mail("mailrs:placement:{user}:*", Shape::Globs(|a| under("mailrs:placement:", a, ":"))),
    mail("mailrs:user:{user}:*", Shape::Globs(|a| under("mailrs:user:", a, ":"))),
    // kept once per message, not per reader: removed when no reader
    // remains, found through the ledger rows the next entry removes
    mail("L:ts:* (no reader left)", Shape::Custom(last_reader::segments)),
    mail("L:t:* (no reader left)", Shape::Custom(last_reader::fingerprints)),
    mail("L:m:{user}|*", Shape::Globs(|a| under("L:m:", a, "|"))),
    mail("L:l:{user}|*", Shape::Globs(|a| under("L:l:", a, "|"))),
    mail("L:f:{user}|*", Shape::Globs(|a| under("L:f:", a, "|"))),
    mail("L:v:{user}|*", Shape::Globs(|a| under("L:v:", a, "|"))),
    mail("L:r:{user}:*", Shape::Globs(|a| under("L:r:", a, ":"))),
    mail("release:facts:{user}:*", Shape::Globs(|a| under("release:facts:", a, ":"))),
    mail("hold:facts:{user}:*", Shape::Globs(|a| under("hold:facts:", a, ":"))),
    mail("rejudge:released:{user}:*", Shape::Globs(|a| under("rejudge:released:", a, ":"))),
    mail("rejudge:recalled:{user}:*", Shape::Globs(|a| under("rejudge:recalled:", a, ":"))),
    mail("rejudge:unheld:{user}:*", Shape::Globs(|a| under("rejudge:unheld:", a, ":"))),
    mail("rejudge:batch:*", Shape::Members { keys: || "rejudge:batch:*".into(), sep: Some("\t") }),
    // what a round gathered to hold, until it settles or its pending batch
    // is decided; the batch counts the purged reader's out, and goes at none
    mail("rejudge:candidates:*", Shape::Custom(rejudge::candidates)),
    mail("mailrs:attpending", Shape::Members { keys: || "mailrs:attpending".into(), sep: Some("\t") }),
    mail("mailrs:attattempts", Shape::Members { keys: || "mailrs:attattempts".into(), sep: Some("\t") }),
    mail("mailrs:attsetaside", Shape::Members { keys: || "mailrs:attsetaside".into(), sep: Some("\t") }),
    mail("mailrs:imgpending", Shape::Members { keys: || "mailrs:imgpending".into(), sep: Some("\t") }),
    mail("mailrs:imgasked", Shape::Members { keys: || "mailrs:imgasked".into(), sep: Some("\t") }),
    mail("mailrs:sync:cursor:{user}", Shape::Key(|a| format!("mailrs:sync:cursor:{a}"))),
    mail("mailrs:mbid:*", Shape::Custom(custom::mailbox_ids)),
    mail("mailrs:alias:v2:* (target)", Shape::Custom(custom::aliases_to)),
    mail("mailrs:account:{user}:perms", Shape::Key(|a| format!("mailrs:account:{a}:perms"))),
    mail("mailrs:account:{user}", Shape::Key(|a| format!("mailrs:account:{a}"))),
];

/// Families that name an account but are kept, and deployment-wide ones
/// the check lists, each with why. The name matches the kept keys, so the
/// purge does not report those that carry the address as unregistered.
#[derive(Debug, Clone, Copy)]
pub struct Kept {
    pub name: &'static str,
    pub why: &'static str,
}

#[rustfmt::skip]
pub const KEPT: &[Kept] = &[
    Kept { name: "admin:audit_log", why: "the audit log is a hash-chained record of facts; the purge itself is one of them" },
    Kept { name: "auth:events:*", why: "the sign-in event log, kept for its retention like the audit log" },
    Kept { name: "auth:alerts:pending", why: "security alerts already raised, mailed to the operator" },
    Kept { name: "owner-notices", why: "notices to the operator, not the user's" },
    Kept { name: "mailrs:fraud:triggers:*", why: "deliveries a rule under review fired on, naming recipients only in values; each day's list expires after 60 days, the time the review needs" },
    Kept { name: "mailrs:masked:submit_log", why: "the log of mail sent from masks, kept like the audit log" },
    Kept { name: "mailrs:outbound:*", why: "mail the user sent that was released for delivery and is still on its way: it is delivered or bounced, then dropped by the queue; mail not yet released is withheld at the delete and purged with mailrs:outbound:withheld:{user}" },
    Kept { name: "shared:{gid}:*", why: "a team's working state (presence, copies, notes, drafts): the group's, not a member's; purged with the group, after the same retention (group_purge)" },
    Kept { name: "mailrs:shared-reply:*", why: "a team's reply claims, keyed by conversation: team working state" },
    Kept { name: "gl:*|{user}", why: "greylisting triplets, keyed by sender and recipient, expire on their own within days" },
    Kept { name: "oidc:pending:*", why: "a sign-in waiting to be linked, keyed by a random handle; expires in 10 minutes and names the address only in its value" },
    Kept { name: "bayes:trained:*", why: "which conversations trained the spam model, by conversation id, not by user" },
    Kept { name: "mailrs:contact:*:{user}", why: "another account's record of its mail with this address: that account's data" },
    Kept { name: "mailrs:callnum:*:{user}", why: "the numbers this address gave another account: that account's data" },
    Kept { name: "L:r:*:{user}", why: "another account's relationship row for this correspondent: that account's data" },
    Kept { name: "mailrs:vacation:*:{user}", why: "another account's memory of having auto-replied to this address: that account's data" },
    Kept { name: "acct:{user}", why: "the SQL core's own cache of the account row, in its process store; dropped at the delete" },
    Kept { name: "perms:{user}", why: "the SQL core's own cache of the account's permissions, in its process store; dropped at the delete" },
    // not anyone's: deployment-wide state the check lists, so a family
    // added to it is classified here too
    Kept { name: "mailrs:dmarcrpt:*", why: "aggregate reports from other domains" },
    Kept { name: "mailrs:tlsrpt:*", why: "TLS reports from other domains" },
    Kept { name: "mailrs:reports:*", why: "abuse reports about this server's senders, by message" },
    Kept { name: "mailrs:bounce:*", why: "bounce records, by outbound message" },
    Kept { name: "mailrs:domfirst:*", why: "when the deployment first saw a domain" },
    Kept { name: "mailrs:fraud:hits:*", why: "rule hit counters by day" },
    Kept { name: "egress:refusals:*", why: "remote refusals by day" },
    Kept { name: "admin:rbl:status", why: "the server's own blocklist standing" },
    Kept { name: "rep:obs", why: "host reputation observations" },
    Kept { name: "scan:hashobs", why: "attachment hash observations" },
    Kept { name: "screening:rules", why: "the screening rules" },
    Kept { name: "aliases_by_target", why: "kevy's index over the alias rows; follows them" },
    Kept { name: "mailrs:alias:sources", why: "the set listing the alias rows; each alias purged leaves it with its row" },
    Kept { name: "notify:new-mail", why: "a pub/sub channel, not a key" },
    Kept { name: "mailrs:events", why: "a pub/sub channel, not a key" },
    Kept { name: "notify:reputation", why: "a pub/sub channel, not a key" },
    Kept { name: "notify:account-signout", why: "a pub/sub channel, not a key" },
    Kept { name: "bayes:tokens:*", why: "the spam model's token counts, not by user" },
    Kept { name: "bayes:triage:*", why: "the triage model, not by user" },
    Kept { name: "mailrs:purge:records", why: "the dropped account record a restore needs, beside the mark; cleared with it" },
    Kept { name: "mailrs:purge:groups", why: "deleted email groups waiting for the purge of their team state, by group id; group_purge clears a group's field last" },
    Kept { name: "mailrs:purge:pending", why: "the purge's own record of who is waiting; the purge clears the address's field last" },
];

/// The spellings of `address` keys may carry: as given, and lowercased.
pub fn spellings(address: &str) -> Vec<String> {
    let lower = address.to_lowercase();
    match lower == address {
        true => vec![lower],
        false => vec![address.to_string(), lower],
    }
}
