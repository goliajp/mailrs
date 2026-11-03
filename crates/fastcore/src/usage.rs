//! What each account's mail takes on disk. The scan is shared with the SQL
//! core and lives in `core_sidestate::families::usage_drift`.

pub use mailrs_core_sidestate::families::usage_drift::maildir_usage;
