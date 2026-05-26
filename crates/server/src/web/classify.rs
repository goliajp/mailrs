//! Mail classification heuristic — category + risk score
//! from sender / subject / body content.

/// classify an email: category + risk score (0=safe .. 100=dangerous)
pub(crate) fn classify_email(
    sender: &str,
    subject: &str,
    text: Option<&str>,
    html: Option<&str>,
) -> (String, u8) {
    // ASCII-fold only: the needle tables below are already lowercase
    // ASCII or CJK (which has no case), so Unicode case-folding adds
    // no matches but burns CPU walking the full Unicode case-fold
    // table on every byte. `to_ascii_lowercase` is a byte-level SIMD
    // path that gives identical match semantics for these needles.
    let sender_lc = sender.to_ascii_lowercase();
    let subject_lc = subject.to_ascii_lowercase();
    let text_lc = text.unwrap_or("").to_ascii_lowercase();
    let html_lc = html.unwrap_or("").to_ascii_lowercase();
    // search the three text fields individually instead of
    // pre-concatenating them — saves a body-sized String alloc per
    // email (text bodies can be 100s of KB) while preserving the
    // "needle hit anywhere" semantics.
    let contains_any = |s: &str| sender_lc.contains(s) || subject_lc.contains(s) || text_lc.contains(s);

    let mut score: i32 = 0;

    // known safe senders (personal, business, dev)
    let safe_domains = [
        "github.com",
        "noreply.github.com",
        "gitlab.com",
        "freee.co.jp",
        "atcoder.jp",
        "apple.com",
        "google.com",
        "golia.jp",
        "golia.ai",
    ];
    let is_safe_domain = safe_domains.iter().any(|d| sender_lc.contains(d));
    if is_safe_domain {
        score -= 30;
    }

    // advertising signals
    let ad_signals = [
        "unsubscribe",
        "配信停止",
        "メール配信",
        "opt-out",
        "list-unsubscribe",
        "配信解除",
        "退订",
        "取消订阅",
        "email preferences",
    ];
    let ad_count = ad_signals
        .iter()
        .filter(|s| contains_any(s) || html_lc.contains(*s))
        .count();

    // newsletter / marketing patterns
    let marketing_signals = [
        "newsletter",
        "ニュースレター",
        "pr】",
        "＜pr＞",
        "お知らせ",
        "セール",
        "キャンペーン",
        "クーポン",
        "ポイント",
        "おすすめ",
        "sale",
        "discount",
        "promotion",
        "deal",
        "offer",
        "特価",
        "限定",
        "タイムセール",
        "お得",
    ];
    let marketing_count = marketing_signals
        .iter()
        .filter(|s| contains_any(s))
        .count();

    // spam signals
    let spam_signals = [
        "click here",
        "act now",
        "limited time",
        "winner",
        "congratulations",
        "lottery",
        "prize",
        "urgent",
        "verify your account",
        "suspended",
        "locked",
        "当選",
        "至急",
        "緊急",
        "中奖",
        "恭喜",
        "紧急",
    ];
    let spam_count = spam_signals.iter().filter(|s| contains_any(s)).count();

    // phishing signals
    let phish_signals = [
        "password",
        "パスワード",
        "密码",
        "login immediately",
        "confirm your identity",
        "アカウントが制限",
        "アカウントを確認",
        "账户异常",
        "账号被锁",
    ];
    let phish_count = phish_signals.iter().filter(|s| contains_any(s)).count();

    // technical signals (tracking pixels, many links, hidden text)
    let has_tracking = html_lc.contains("width=\"1\"")
        || html_lc.contains("width:1px")
        || html_lc.contains("height=\"1\"")
        || html_lc.contains("height:1px");
    let link_count = html_lc.matches("<a ").count();

    score += ad_count as i32 * 5;
    score += marketing_count as i32 * 8;
    score += spam_count as i32 * 20;
    score += phish_count as i32 * 25;
    if has_tracking {
        score += 5;
    }
    if link_count > 20 {
        score += 5;
    }

    // known notification senders (low risk)
    let notification_domains = [
        "facebookmail.com",
        "linkedin.com",
        "substack.com",
        "steampowered.com",
        "quora.com",
        "tripadvisor.com",
        "noreply@",
        "no-reply@",
        "notification",
    ];
    let is_notification = notification_domains.iter().any(|d| sender_lc.contains(d));
    if is_notification && score < 30 {
        score = score.min(15);
    }

    let score = score.clamp(0, 100) as u8;

    let category = if score >= 60 {
        "scam"
    } else if score >= 40 {
        "spam"
    } else if ad_count > 0 || marketing_count >= 2 || has_tracking {
        "promotion"
    } else if is_notification {
        "notification"
    } else if is_safe_domain || score == 0 {
        "personal"
    } else {
        "general"
    };

    (category.to_string(), score)
}
