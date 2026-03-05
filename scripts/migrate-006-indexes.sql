-- additional indexes for common query patterns

-- accounts by domain (for domain-level queries and super_domains)
CREATE INDEX IF NOT EXISTS idx_accounts_domain ON accounts(domain);

-- messages by sender (for contact search)
CREATE INDEX IF NOT EXISTS idx_messages_sender ON messages(sender);

-- outbound queue by domain (for per-domain delivery grouping)
CREATE INDEX IF NOT EXISTS idx_queue_domain ON outbound_queue(domain) WHERE status = 'pending';

-- greylist cleanup (for time-based expiry)
CREATE INDEX IF NOT EXISTS idx_greylist_last_seen ON greylist_triplets(last_seen);

-- email_analysis by risk_score (for security dashboard)
CREATE INDEX IF NOT EXISTS idx_ea_risk ON email_analysis(risk_score) WHERE risk_score > 0;
