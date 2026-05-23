-- TLSRPT event facts (RFC 8460 §4.3).
--
-- Append-only event facts that feed the daily report builder. One
-- row per outbound STARTTLS attempt (success or failure). Daily
-- flush task DELETEs rows in the drained window after building +
-- submitting the report — see crates/server/src/outbound_tls_rpt.rs
-- and `mailrs_tls_rpt::store::Store::drain_window`.
--
-- Facts vs derivations: this table is FACTS (append-only). The
-- report itself is a derivation, recomputable from the facts at
-- any time; we don't store reports separately because once a
-- report is submitted, the facts are no longer needed.

CREATE TABLE IF NOT EXISTS tls_rpt_events (
    id BIGSERIAL PRIMARY KEY,

    -- When the observation happened (epoch seconds, UTC).
    recorded_at_unix BIGINT NOT NULL,

    -- 'success' or 'failure' — discriminates the kind column below.
    kind TEXT NOT NULL CHECK (kind IN ('success', 'failure')),

    -- The receiving domain the TLS attempt was for. Matches the
    -- TLSRPT report's `policy.policy-domain` field exactly.
    policy_domain TEXT NOT NULL,

    -- 'sts', 'tlsa', or 'no-policy-found' — kebab-case per RFC 8460
    -- §4.2. Stored verbatim for direct round-trip into the report.
    policy_type TEXT NOT NULL,

    -- MX hostname connected to (NULL only on the rare
    -- failure-before-MX-known path).
    mx_host TEXT,

    -- Success-row fields are all NULL.
    -- Failure-row fields: result_type is the RFC 8460 §4.3 string
    -- (kebab-case, e.g. 'certificate-host-mismatch').
    result_type TEXT,
    sending_mta_ip TEXT,
    receiving_ip TEXT,
    receiving_mx_helo TEXT,
    additional_information TEXT,
    failure_reason_code TEXT
);

-- Drain queries scan by recorded_at_unix; the index keeps the
-- daily flush O(rows-in-window) rather than O(all-rows).
CREATE INDEX IF NOT EXISTS tls_rpt_events_recorded_at_idx
    ON tls_rpt_events (recorded_at_unix);
