//! Params for the sender and push tools (batch 18).

use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReclassifySenderParams {
    /// The sender's address.
    pub sender: String,
    /// Where this sender's conversations belong: `inbox`, `notifications`
    /// or `promotions`.
    pub bucket: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CorrectionIdParams {
    /// The correction's id, from reclassify_sender.
    pub correction_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SenderRuleParams {
    /// The sender's address.
    pub address: String,
    /// Where the sender's mail goes: `inbox`, `notifications` or `promotions`.
    #[serde(default)]
    pub bucket: Option<String>,
    /// Keep only this many of the sender's newest conversations (1 to 10000).
    #[serde(default)]
    pub keep_latest: Option<u32>,
    /// Let go of the sender's conversations older than this (1 to 3650 days).
    #[serde(default)]
    pub older_than_days: Option<u32>,
    /// What letting go means: `archive` (default) or `trash`.
    #[serde(default)]
    pub disposal: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SenderAddressParams {
    /// The sender's address.
    pub address: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NotifySettingsParams {
    /// Which mail pushes: `default` (mail to answer and important contacts),
    /// `all`, `important` or `off`.
    pub mode: String,
    /// Quiet hours start, minutes after local midnight; set both or neither.
    #[serde(default)]
    pub quiet_start: Option<u16>,
    /// Quiet hours end, minutes after local midnight.
    #[serde(default)]
    pub quiet_end: Option<u16>,
    /// The reader's offset from UTC in minutes, for the quiet hours.
    #[serde(default)]
    pub utc_offset_minutes: i16,
    /// Say only "new mail", without sender or subject.
    #[serde(default)]
    pub hide_content: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SenderNotifyParams {
    /// The sender's address.
    pub address: String,
    /// `always`, `never`, or absent to remove the choice.
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ThreadNotifyParams {
    pub thread_id: String,
    /// `follow`, `mute`, or absent to remove the choice.
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NotifyStatsParams {
    /// How many days back, newest first (1 to 90, default 7).
    #[serde(default)]
    pub days: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportantContactParams {
    /// The contact's address.
    pub address: String,
    /// True marks the contact important; false removes the mark.
    pub important: bool,
}
