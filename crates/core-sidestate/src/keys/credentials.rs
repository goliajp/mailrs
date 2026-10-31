//! Keys for an account's other credentials: agent keys, its second factor
//! and its recovery links. webapi issues and checks them; clearing a
//! password reads them from either core.

/// Hash of the agent keys `owner` holds, id → record.
pub fn agent_keys_key(owner: &str) -> String {
    format!("agent:keys:{owner}")
}

/// Hash of when each of `owner`'s agent keys was last used.
pub fn agent_keys_last_used_key(owner: &str) -> String {
    format!("agent:keys:last_used:{owner}")
}

/// Counter that numbers `owner`'s agent keys.
pub fn agent_keys_counter_key(owner: &str) -> String {
    format!("agent:keys:counter:{owner}")
}

/// Hash of `address`'s second factor.
pub fn totp_key(address: &str) -> String {
    format!("totp:{address}")
}

/// A mailed recovery link, by its token.
pub fn pwreset_key(token: &str) -> String {
    format!("pwreset:{token}")
}

/// The recovery link `address` was last sent.
pub fn pwreset_by_addr_key(address: &str) -> String {
    format!("pwreset_by_addr:{address}")
}

/// How often `address` asked for a recovery link.
pub fn pwreset_ratelimit_key(address: &str) -> String {
    format!("pwreset:ratelimit:{address}")
}
