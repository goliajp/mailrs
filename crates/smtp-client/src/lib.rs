pub mod connection;
pub mod dane;
pub mod mx;
pub mod response;

pub use connection::{SmtpConnection, TimeoutConfig};
pub use dane::{dane_tls_config, resolve_tlsa, DaneVerifier, TlsaRecord};
pub use mx::{fallback_to_domain, resolve_mx, sort_mx_records, MxCache, MxRecord, TokioResolver};
pub use response::{parse_response, SmtpResponse};
