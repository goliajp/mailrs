//! The per-sender relationship record.

use serde::{Deserialize, Serialize};

/// One sender's relationship record, as importance scoring reads it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq)]
pub struct ContactScoring {
    /// True if both sides have sent.
    pub is_mutual: bool,
    /// True if mailing-list-like sender.
    pub is_mailing_list: bool,
    /// True if marked VIP by user.
    pub is_vip: bool,
    /// True if blocked.
    pub is_blocked: bool,
    /// Manual importance bias in [-1.0, 1.0].
    pub importance_bias: f32,
    /// Count of inbound messages.
    pub received_count: u32,
    /// Count of outbound messages.
    pub sent_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoring_defaults() {
        let s = ContactScoring::default();
        assert!(!s.is_mutual);
        assert_eq!(s.received_count, 0);
    }
}
