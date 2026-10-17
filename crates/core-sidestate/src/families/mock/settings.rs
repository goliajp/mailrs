//! What an admin can change about mock mail, read by every process from
//! one hash so a change takes effect without a restart.
//!
//! ```text
//! mailrs:mock:settings   hash  enabled, retention_days, max_messages,
//!                              max_total_mb, max_namespaces_per_user, create
//! ```
//!
//! An absent field is its default, so a deployment that never saved
//! settings behaves as it did before they existed.

use std::time::Duration;

use kevy_client::Connection;
use serde::{Deserialize, Serialize};

use super::*;

const KEY: &[u8] = b"mailrs:mock:settings";

/// Who may create namespaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CreatePolicy {
    AllUsers,
    AdminsOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// off: new mail to the mock domain is refused; stored mail stays readable
    pub enabled: bool,
    pub retention_days: u32,
    pub max_messages: usize,
    pub max_total_mb: u64,
    pub max_namespaces_per_user: usize,
    pub create: CreatePolicy,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: 3,
            max_messages: 500,
            max_total_mb: 2048,
            max_namespaces_per_user: 10,
            create: CreatePolicy::AllUsers,
        }
    }
}

/// The bounds a saved value must fall in.
pub const RETENTION_DAYS: std::ops::RangeInclusive<u32> = 1..=30;
pub const MAX_MESSAGES_RANGE: std::ops::RangeInclusive<usize> = 10..=5000;
pub const MAX_TOTAL_MB: std::ops::RangeInclusive<u64> = 64..=20_480;
pub const NAMESPACES_PER_USER: std::ops::RangeInclusive<usize> = 1..=100;

impl Settings {
    pub fn retention(&self) -> Duration {
        Duration::from_secs(u64::from(self.retention_days) * 86_400)
    }

    pub fn max_total_bytes(&self) -> i64 {
        (self.max_total_mb as i64) * 1024 * 1024
    }

    /// The first value outside its bounds, as a message for the admin.
    pub fn invalid(&self) -> Option<String> {
        let out = |name: &str, lo: String, hi: String| {
            Some(format!("{name} must be between {lo} and {hi}"))
        };
        if !RETENTION_DAYS.contains(&self.retention_days) {
            return out(
                "retention_days",
                RETENTION_DAYS.start().to_string(),
                RETENTION_DAYS.end().to_string(),
            );
        }
        if !MAX_MESSAGES_RANGE.contains(&self.max_messages) {
            return out(
                "max_messages",
                MAX_MESSAGES_RANGE.start().to_string(),
                MAX_MESSAGES_RANGE.end().to_string(),
            );
        }
        if !MAX_TOTAL_MB.contains(&self.max_total_mb) {
            return out(
                "max_total_mb",
                MAX_TOTAL_MB.start().to_string(),
                MAX_TOTAL_MB.end().to_string(),
            );
        }
        if !NAMESPACES_PER_USER.contains(&self.max_namespaces_per_user) {
            return out(
                "max_namespaces_per_user",
                NAMESPACES_PER_USER.start().to_string(),
                NAMESPACES_PER_USER.end().to_string(),
            );
        }
        None
    }
}

pub fn settings(conn: &mut Connection) -> MockResult<Settings> {
    let mut s = Settings::default();
    let fields = conn.hgetall(KEY)?;
    for pair in fields.chunks_exact(2) {
        let v = utf8(pair[1].clone());
        match pair[0].as_slice() {
            b"enabled" => s.enabled = v != "false",
            b"retention_days" => s.retention_days = v.parse().unwrap_or(s.retention_days),
            b"max_messages" => s.max_messages = v.parse().unwrap_or(s.max_messages),
            b"max_total_mb" => s.max_total_mb = v.parse().unwrap_or(s.max_total_mb),
            b"max_namespaces_per_user" => {
                s.max_namespaces_per_user = v.parse().unwrap_or(s.max_namespaces_per_user)
            }
            b"create" if v == "admins-only" => s.create = CreatePolicy::AdminsOnly,
            _ => {}
        }
    }
    Ok(s)
}

pub fn save_settings(conn: &mut Connection, s: &Settings) -> MockResult<()> {
    if let Some(why) = s.invalid() {
        return Err(MockError::BadSetting(why));
    }
    let create = match s.create {
        CreatePolicy::AllUsers => "all-users",
        CreatePolicy::AdminsOnly => "admins-only",
    };
    conn.hset(
        KEY,
        &[
            (b"enabled".as_slice(), s.enabled.to_string().as_bytes()),
            (b"retention_days", s.retention_days.to_string().as_bytes()),
            (b"max_messages", s.max_messages.to_string().as_bytes()),
            (b"max_total_mb", s.max_total_mb.to_string().as_bytes()),
            (
                b"max_namespaces_per_user",
                s.max_namespaces_per_user.to_string().as_bytes(),
            ),
            (b"create", create.as_bytes()),
        ],
    )?;
    Ok(())
}
