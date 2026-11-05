//! This process's parse-worker health, as the health probes carry it.

use mailrs_core_api::types::{ParserFailureWire, ParsingHealthWire};

/// The snapshot `mailrs_parse_worker` keeps, on the wire.
pub fn health() -> ParsingHealthWire {
    let s = mailrs_parse_worker::health::snapshot();
    ParsingHealthWire {
        worker: s.worker,
        failures_24h: s.failures_24h,
        last_failure_at: s.last_failure_at,
        worker_restarts: s.worker_restarts,
        failures: s
            .failures
            .into_iter()
            .map(|((parser, kind), count)| ParserFailureWire {
                parser: parser.to_string(),
                kind: kind.to_string(),
                count,
            })
            .collect(),
    }
}
