// importance scoring engine: determines email value/priority

/// importance level for a message
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImportanceLevel {
    Critical,
    Important,
    Normal,
    Low,
    Noise,
}

impl ImportanceLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::Important => "important",
            Self::Normal => "normal",
            Self::Low => "low",
            Self::Noise => "noise",
        }
    }

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

/// signals contributing to the importance score
#[derive(Debug, Clone)]
pub(crate) struct ImportanceSignals {
    pub is_mutual_contact: bool,
    pub is_direct_recipient: bool,
    pub is_reply_to_my_email: bool,
    pub has_action_items: bool,
    pub is_vip_sender: bool,
    pub is_bulk_sender: bool,
    pub is_mailing_list: bool,
    pub is_automated: bool,
    pub has_tracking_pixel: bool,
    pub is_template_heavy: bool,
    pub text_to_html_ratio: f32,
    pub link_count: usize,
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

/// calculate importance score from signals
pub(crate) fn calculate_importance(signals: &ImportanceSignals) -> (ImportanceLevel, f32) {
    let mut score: f32 = 0.3; // baseline: normal

    // positive signals
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

    // negative signals
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

    // user bias
    score += signals.contact_importance_bias;

    // clamp to [-0.5, 1.0]
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
        assert_eq!(ImportanceLevel::from_score(0.79), ImportanceLevel::Important);
        assert_eq!(ImportanceLevel::from_score(0.5), ImportanceLevel::Important);
        assert_eq!(ImportanceLevel::from_score(0.49), ImportanceLevel::Normal);
        assert_eq!(ImportanceLevel::from_score(0.2), ImportanceLevel::Normal);
        assert_eq!(ImportanceLevel::from_score(0.19), ImportanceLevel::Low);
        assert_eq!(ImportanceLevel::from_score(0.0), ImportanceLevel::Low);
        assert_eq!(ImportanceLevel::from_score(-0.01), ImportanceLevel::Noise);
    }
}
