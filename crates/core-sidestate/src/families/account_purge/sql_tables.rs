//! The SQL core's part of the registry: the tables that hold an account's
//! rows, which that core purges by table rather than by key.

/// The SQL core's rows an account owns that no foreign key ties to the
/// account row (`init-schema.sql`): table, column. The purge deletes
/// them, then the row, which takes [`SQL_CASCADED`] with it.
pub const SQL_TABLES: &[(&str, &str)] = &[
    ("account_send_as", "account_address"),
    ("user_uids", "user_address"),
    ("email_contacts", "user_address"),
    ("sender_feedback", "user_address"),
    ("snoozed_conversations", "account_address"),
    ("message_placements", "account_address"),
    ("quarantine_holds", "account_address"),
    ("release_facts", "account_address"),
    ("hold_facts", "account_address"),
    ("trashed_conversations", "account_address"),
    ("accepted_senders", "account_address"),
    ("shared_thread_copies", "account_address"),
    ("thread_labels", "account_address"),
    ("labels", "account_address"),
    ("webhook_subscriptions", "account_address"),
    ("drafts", "user_address"),
    ("spam_feedback", "user_address"),
    ("vacation_dedup", "recipient"),
    ("aliases", "target_address"),
    // last: its messages, their attachment text and analysis cascade
    ("mailboxes", "user_address"),
];

/// The SQL core's rows that reference the account row by a cascading
/// foreign key: kept while the row is marked deleted, so a restore brings
/// them back, and gone with the row at the purge. Counted by the purge;
/// table, column. Their own dependents (calendar events, contacts, an
/// app's keys) cascade with them.
pub const SQL_CASCADED: &[(&str, &str)] = &[
    ("sieve_scripts", "address"),
    ("email_templates", "user_address"),
    ("account_groups", "account_address"),
    ("account_permission_overrides", "account_address"),
    ("apps", "owner_address"),
    ("api_keys", "account_address"),
    ("email_group_members", "member_address"),
    ("signatures", "account_address"),
    ("password_reset_tokens", "account_address"),
    ("totp_secrets", "account_address"),
    ("encryption_keys", "account_address"),
    ("calendars", "account_address"),
    ("address_books", "account_address"),
    ("oauth_auth_codes", "account_address"),
    ("oauth_refresh_tokens", "account_address"),
    ("external_calendar_feeds", "account_address"),
];
