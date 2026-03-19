-- webhook tables (were in init-schema but missing from existing deployments)
CREATE TABLE IF NOT EXISTS webhook_subscriptions (
    id BIGSERIAL PRIMARY KEY,
    account_address TEXT NOT NULL,
    url TEXT NOT NULL,
    event_type TEXT NOT NULL DEFAULT 'new_message',
    filter_sender TEXT,
    filter_thread_id TEXT,
    signing_secret TEXT NOT NULL,
    active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_webhook_subs_account ON webhook_subscriptions(account_address) WHERE active = true;
CREATE INDEX IF NOT EXISTS idx_webhook_subs_event ON webhook_subscriptions(event_type, active) WHERE active = true;

CREATE TABLE IF NOT EXISTS webhook_outbox (
    id BIGSERIAL PRIMARY KEY,
    subscription_id BIGINT NOT NULL REFERENCES webhook_subscriptions(id) ON DELETE CASCADE,
    payload JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 8,
    next_retry BIGINT NOT NULL,
    last_error TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_webhook_outbox_pending ON webhook_outbox(status, next_retry)
    WHERE status = 'pending';
