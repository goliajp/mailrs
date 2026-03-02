pub mod connection;
pub mod mx;
pub mod response;

pub use connection::{SmtpConnection, TimeoutConfig};
pub use mx::{fallback_to_domain, resolve_mx, sort_mx_records, MxCache, MxRecord, TokioResolver};
pub use response::{parse_response, SmtpResponse};
