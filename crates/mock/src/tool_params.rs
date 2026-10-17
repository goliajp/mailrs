//! Arguments of the `mock_*` MCP tools. Every field's doc comment becomes
//! the tool schema an agent reads. A tool that works on a namespace names
//! it `namespace`: a key scoped to one namespace is checked against it.

use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MockCreateNamespaceParams {
    /// 3 to 32 characters of a-z, 0-9 and '-'. Global across users, so
    /// prefix it with something of yours (e.g. "myapp-ci").
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MockListNamespacesParams {
    /// Admins only: list every user's namespaces.
    #[serde(default)]
    pub all: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MockNamespaceParams {
    /// Namespace name.
    pub namespace: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MockNewAddressParams {
    /// Namespace name.
    pub namespace: String,
    /// Tag to use (a-z, 0-9, '.', '_', '-', up to 31). Omit for a random one.
    #[serde(default)]
    pub tag: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MockListMessagesParams {
    /// Namespace name.
    pub namespace: String,
    /// Only this exact tag.
    #[serde(default)]
    pub tag: Option<String>,
    /// Only tags starting with this.
    #[serde(default)]
    pub tag_prefix: Option<String>,
    /// Only mail addressed to this address (envelope, To or Cc).
    #[serde(default)]
    pub to: Option<String>,
    /// Only mail whose From contains this, case-insensitive.
    #[serde(default)]
    pub from: Option<String>,
    /// Only mail whose subject contains this, case-insensitive.
    #[serde(default)]
    pub subject: Option<String>,
    /// Only mail that arrived at or after this time, ms since the epoch.
    #[serde(default)]
    pub since: Option<i64>,
    /// Max messages, newest first. Defaults to 20, caps at 500.
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MockWaitForMessageParams {
    /// Namespace name.
    pub namespace: String,
    /// Wait for mail to this tag. With a tag minted by mock_new_address and
    /// no `since`, mail that arrived after the address was minted counts.
    #[serde(default)]
    pub tag: Option<String>,
    /// Only mail whose From contains this, case-insensitive.
    #[serde(default)]
    pub from: Option<String>,
    /// Only mail whose subject contains this, case-insensitive.
    #[serde(default)]
    pub subject: Option<String>,
    /// Only mail that arrived at or after this time, ms since the epoch.
    /// Defaults to when the tag was minted, else to now.
    #[serde(default)]
    pub since: Option<i64>,
    /// Seconds to wait. Defaults to 30, caps at 60.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MockReadMessageParams {
    /// Namespace name.
    pub namespace: String,
    /// Message id from mock_list_messages or mock_wait_for_message.
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MockDeleteMessagesParams {
    /// Namespace name.
    pub namespace: String,
    /// Delete only this message.
    #[serde(default)]
    pub id: Option<String>,
    /// Delete only messages with this tag. With neither id nor tag, every
    /// message of the namespace is deleted.
    #[serde(default)]
    pub tag: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MockSendParams {
    /// Namespace to send from; the copy lands in its captured outbox.
    pub namespace: String,
    /// Tag of the sending address. Defaults to "sender".
    #[serde(default)]
    pub from_tag: Option<String>,
    /// Recipients. Only mock addresses receive it; every other address is
    /// recorded in the captured copy and never contacted.
    pub to: Vec<String>,
    pub subject: String,
    /// Plain-text body.
    pub text: String,
    /// Optional HTML body.
    #[serde(default)]
    pub html: Option<String>,
}
