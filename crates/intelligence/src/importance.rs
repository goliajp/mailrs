//! Heuristic importance scoring (no LLM).
//!
//! Computes an importance score in `[-0.5, 1.0]` from boolean / numeric
//! signals about a message, then maps it to a five-level enum
//! ([`ImportanceLevel`]) for display and filtering.

/// Importance level for a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportanceLevel {
    /// Score ≥ 0.8 — surface prominently.
    Critical,
    /// Score 0.5-0.8 — important.
    Important,
    /// Score 0.2-0.5 — normal inbox priority.
    Normal,
    /// Score 0.0-0.2 — low priority (newsletters, notifications).
    Low,
    /// Score < 0 — noise; safe to demote / archive.
    Noise,
}

impl ImportanceLevel {
    /// Lower-snake-case rendering for serialization.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::Important => "important",
            Self::Normal => "normal",
            Self::Low => "low",
            Self::Noise => "noise",
        }
    }

    /// Bucket a numeric score into a level. Score is expected in `[-0.5, 1.0]`.
    pub fn from_score(score: f32) -> Self {
        if score >= 0.8 {
            Self::Critical
        } else if score >= 0.5 {
            Self::Important
        } else if score >= 0.2 {
            Self::Normal
        } else if score >= 0.0 {
            Self::Low
        } else {
            Self::Noise
        }
    }
}

/// Signals contributing to the importance score.
#[derive(Debug, Clone)]
pub struct ImportanceSignals {
    /// Sender has been emailed by the user before (mutual relationship).
    pub is_mutual_contact: bool,
    /// User is in `To:` (not `Cc:` or `Bcc:`).
    pub is_direct_recipient: bool,
    /// Message references one the user previously sent.
    pub is_reply_to_my_email: bool,
    /// LLM analysis surfaced one or more action items.
    pub has_action_items: bool,
    /// Sender is on the user's explicit VIP list.
    pub is_vip_sender: bool,
    /// `List-*` headers indicate mailing-list / bulk traffic.
    pub is_bulk_sender: bool,
    /// Specifically a mailing-list message (List-Id present).
    pub is_mailing_list: bool,
    /// Sender local-part matches `no-reply@` / `notification@` / etc.
    pub is_automated: bool,
    /// Tracking pixel was found in the body.
    pub has_tracking_pixel: bool,
    /// HTML is mostly chrome — table layout, lots of inline styles.
    pub is_template_heavy: bool,
    /// Ratio of plain-text bytes to total HTML bytes.
    pub text_to_html_ratio: f32,
    /// Count of `<a>` tags in the body.
    pub link_count: usize,
    /// Manual per-contact bias from the user's address book.
    pub contact_importance_bias: f32,
}

impl Default for ImportanceSignals {
    fn default() -> Self {
        Self {
            is_mutual_contact: false,
            is_direct_recipient: false,
            is_reply_to_my_email: false,
            has_action_items: false,
            is_vip_sender: false,
            is_bulk_sender: false,
            is_mailing_list: false,
            is_automated: false,
            has_tracking_pixel: false,
            is_template_heavy: false,
            text_to_html_ratio: 1.0,
            link_count: 0,
            contact_importance_bias: 0.0,
        }
    }
}

/// Calculate importance score from signals.
pub fn calculate_importance(signals: &ImportanceSignals) -> (ImportanceLevel, f32) {
    let mut score: f32 = 0.3; // baseline: normal

    if signals.is_mutual_contact {
        score += 0.3;
    }
    if signals.is_direct_recipient {
        score += 0.1;
    }
    if signals.is_reply_to_my_email {
        score += 0.3;
    }
    if signals.has_action_items {
        score += 0.2;
    }
    if signals.is_vip_sender {
        score += 0.4;
    }

    if signals.is_bulk_sender {
        score -= 0.3;
    }
    if signals.is_mailing_list {
        score -= 0.2;
    }
    if signals.is_automated {
        score -= 0.3;
    }
    if signals.has_tracking_pixel {
        score -= 0.1;
    }
    if signals.is_template_heavy {
        score -= 0.2;
    }
    if signals.link_count > 20 {
        score -= 0.1;
    }

    score += signals.contact_importance_bias;

    score = score.clamp(-0.5, 1.0);

    let level = ImportanceLevel::from_score(score);
    (level, score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_score_is_normal() {
        let signals = ImportanceSignals::default();
        let (level, score) = calculate_importance(&signals);
        assert_eq!(level, ImportanceLevel::Normal);
        assert!((score - 0.3).abs() < 0.001);
    }

    #[test]
    fn mutual_contact_direct_is_important() {
        let signals = ImportanceSignals {
            is_mutual_contact: true,
            is_direct_recipient: true,
            ..Default::default()
        };
        let (level, _) = calculate_importance(&signals);
        assert!(level == ImportanceLevel::Important || level == ImportanceLevel::Critical);
    }

    #[test]
    fn vip_reply_is_critical() {
        let signals = ImportanceSignals {
            is_vip_sender: true,
            is_reply_to_my_email: true,
            is_mutual_contact: true,
            ..Default::default()
        };
        let (level, score) = calculate_importance(&signals);
        assert_eq!(level, ImportanceLevel::Critical);
        assert!(score >= 0.8);
    }

    #[test]
    fn bulk_automated_is_low_or_noise() {
        let signals = ImportanceSignals {
            is_bulk_sender: true,
            is_automated: true,
            is_mailing_list: true,
            has_tracking_pixel: true,
            ..Default::default()
        };
        let (level, _) = calculate_importance(&signals);
        assert!(level == ImportanceLevel::Low || level == ImportanceLevel::Noise);
    }

    #[test]
    fn template_heavy_marketing_is_low() {
        let signals = ImportanceSignals {
            is_bulk_sender: true,
            has_tracking_pixel: true,
            is_template_heavy: true,
            link_count: 30,
            ..Default::default()
        };
        let (level, _) = calculate_importance(&signals);
        assert!(level == ImportanceLevel::Low || level == ImportanceLevel::Noise);
    }

    #[test]
    fn user_bias_positive_boosts() {
        let signals = ImportanceSignals {
            contact_importance_bias: 0.5,
            ..Default::default()
        };
        let (level, _) = calculate_importance(&signals);
        assert_eq!(level, ImportanceLevel::Critical);
    }

    #[test]
    fn user_bias_negative_demotes() {
        let signals = ImportanceSignals {
            is_mutual_contact: true,
            contact_importance_bias: -0.5,
            ..Default::default()
        };
        let (level, _) = calculate_importance(&signals);
        assert!(level == ImportanceLevel::Normal || level == ImportanceLevel::Low);
    }

    #[test]
    fn score_clamped_to_range() {
        let signals = ImportanceSignals {
            is_vip_sender: true,
            is_mutual_contact: true,
            is_reply_to_my_email: true,
            is_direct_recipient: true,
            has_action_items: true,
            contact_importance_bias: 1.0,
            ..Default::default()
        };
        let (_, score) = calculate_importance(&signals);
        assert!(score <= 1.0);

        let signals2 = ImportanceSignals {
            is_bulk_sender: true,
            is_automated: true,
            is_mailing_list: true,
            has_tracking_pixel: true,
            is_template_heavy: true,
            contact_importance_bias: -1.0,
            ..Default::default()
        };
        let (_, score2) = calculate_importance(&signals2);
        assert!(score2 >= -0.5);
    }

    #[test]
    fn importance_level_as_str() {
        assert_eq!(ImportanceLevel::Critical.as_str(), "critical");
        assert_eq!(ImportanceLevel::Important.as_str(), "important");
        assert_eq!(ImportanceLevel::Normal.as_str(), "normal");
        assert_eq!(ImportanceLevel::Low.as_str(), "low");
        assert_eq!(ImportanceLevel::Noise.as_str(), "noise");
    }

    #[test]
    fn importance_level_from_score_boundaries() {
        assert_eq!(ImportanceLevel::from_score(1.0), ImportanceLevel::Critical);
        assert_eq!(ImportanceLevel::from_score(0.8), ImportanceLevel::Critical);
        assert_eq!(
            ImportanceLevel::from_score(0.79),
            ImportanceLevel::Important
        );
        assert_eq!(ImportanceLevel::from_score(0.5), ImportanceLevel::Important);
        assert_eq!(ImportanceLevel::from_score(0.49), ImportanceLevel::Normal);
        assert_eq!(ImportanceLevel::from_score(0.2), ImportanceLevel::Normal);
        assert_eq!(ImportanceLevel::from_score(0.19), ImportanceLevel::Low);
        assert_eq!(ImportanceLevel::from_score(0.0), ImportanceLevel::Low);
        assert_eq!(ImportanceLevel::from_score(-0.01), ImportanceLevel::Noise);
    }

    // ===== Additional corner-case tests =====

    #[test]
    fn default_signals_have_text_to_html_ratio_one() {
        // Sanity check on default — should be 1.0 (all-text), not 0.0.
        let d = ImportanceSignals::default();
        assert!((d.text_to_html_ratio - 1.0).abs() < f32::EPSILON);
        assert_eq!(d.link_count, 0);
        assert!((d.contact_importance_bias).abs() < f32::EPSILON);
    }

    #[test]
    fn link_count_threshold_exactly_20_does_not_penalize() {
        // The penalty triggers only when link_count > 20 (strict >).
        let s = ImportanceSignals {
            link_count: 20,
            ..Default::default()
        };
        let (_, score) = calculate_importance(&s);
        // Score should be the baseline 0.3 (no penalty applied).
        assert!((score - 0.3).abs() < 0.001);
    }

    #[test]
    fn link_count_above_threshold_penalizes() {
        let s = ImportanceSignals {
            link_count: 21,
            ..Default::default()
        };
        let (_, score) = calculate_importance(&s);
        // baseline (0.3) - 0.1 = 0.2
        assert!((score - 0.2).abs() < 0.001);
    }

    #[test]
    fn from_score_negative_clamp_lower_bound() {
        // anything strictly below 0.0 → Noise, even very negative numbers.
        assert_eq!(ImportanceLevel::from_score(-0.5), ImportanceLevel::Noise);
        assert_eq!(ImportanceLevel::from_score(-100.0), ImportanceLevel::Noise);
        assert_eq!(
            ImportanceLevel::from_score(f32::NEG_INFINITY),
            ImportanceLevel::Noise
        );
    }

    #[test]
    fn from_score_above_one_still_critical() {
        // even out-of-range high values map to Critical.
        assert_eq!(ImportanceLevel::from_score(2.0), ImportanceLevel::Critical);
        assert_eq!(
            ImportanceLevel::from_score(f32::INFINITY),
            ImportanceLevel::Critical
        );
    }

    #[test]
    fn from_score_nan_behavior_is_noise() {
        // NaN comparisons are always false → falls through to Noise.
        assert_eq!(
            ImportanceLevel::from_score(f32::NAN),
            ImportanceLevel::Noise
        );
    }

    #[test]
    fn signals_with_all_positives_capped_at_one() {
        // every positive signal + max bias still clamps to 1.0.
        let s = ImportanceSignals {
            is_mutual_contact: true,
            is_direct_recipient: true,
            is_reply_to_my_email: true,
            has_action_items: true,
            is_vip_sender: true,
            contact_importance_bias: 10.0, // intentionally huge
            ..Default::default()
        };
        let (level, score) = calculate_importance(&s);
        assert!((score - 1.0).abs() < f32::EPSILON);
        assert_eq!(level, ImportanceLevel::Critical);
    }

    #[test]
    fn signals_with_all_negatives_capped_at_minus_half() {
        // every negative signal + max negative bias clamps to -0.5.
        let s = ImportanceSignals {
            is_bulk_sender: true,
            is_mailing_list: true,
            is_automated: true,
            has_tracking_pixel: true,
            is_template_heavy: true,
            link_count: 100,
            contact_importance_bias: -10.0,
            ..Default::default()
        };
        let (level, score) = calculate_importance(&s);
        assert!((score - (-0.5)).abs() < f32::EPSILON);
        assert_eq!(level, ImportanceLevel::Noise);
    }

    #[test]
    fn direct_recipient_alone_does_not_promote_to_important() {
        // baseline 0.3 + 0.1 = 0.4 → still Normal.
        let s = ImportanceSignals {
            is_direct_recipient: true,
            ..Default::default()
        };
        let (level, score) = calculate_importance(&s);
        assert!((score - 0.4).abs() < 0.001);
        assert_eq!(level, ImportanceLevel::Normal);
    }

    #[test]
    fn signals_cloning_preserves_all_fields() {
        let s = ImportanceSignals {
            is_mutual_contact: true,
            is_direct_recipient: true,
            is_vip_sender: true,
            text_to_html_ratio: 0.5,
            link_count: 7,
            contact_importance_bias: 0.25,
            ..Default::default()
        };
        let c = s.clone();
        assert_eq!(c.is_mutual_contact, s.is_mutual_contact);
        assert_eq!(c.is_direct_recipient, s.is_direct_recipient);
        assert_eq!(c.is_vip_sender, s.is_vip_sender);
        assert!((c.text_to_html_ratio - s.text_to_html_ratio).abs() < f32::EPSILON);
        assert_eq!(c.link_count, s.link_count);
        assert!((c.contact_importance_bias - s.contact_importance_bias).abs() < f32::EPSILON);
    }

    #[test]
    fn importance_level_copy_trait() {
        // Ensure ImportanceLevel is Copy — important for caller ergonomics.
        let l = ImportanceLevel::Critical;
        let copy = l;
        // both still usable
        assert_eq!(l, copy);
        assert_eq!(copy.as_str(), "critical");
    }

    #[test]
    fn calculate_importance_returns_consistent_level_and_score() {
        // For any signals, the returned level must be exactly from_score(returned_score).
        let cases = [
            ImportanceSignals::default(),
            ImportanceSignals {
                is_vip_sender: true,
                ..Default::default()
            },
            ImportanceSignals {
                is_bulk_sender: true,
                contact_importance_bias: -0.1,
                ..Default::default()
            },
        ];
        for s in cases {
            let (level, score) = calculate_importance(&s);
            assert_eq!(
                level,
                ImportanceLevel::from_score(score),
                "level and score must agree (score={score})",
            );
        }
    }
}
