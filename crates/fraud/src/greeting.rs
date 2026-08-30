//! Mail that greets somebody else by name.
//!
//! ```text
//! 兰静思，恭喜！你的中年翻身仗就要打响了！
//! 兰静思，您命中天乙贵人坐配偶……
//! ```
//!
//! The reader is 李好. 兰静思 is a stranger, and every one of these
//! messages opens by addressing her.
//!
//! # Why this is intrinsic and not a guess
//!
//! A sender who greets you by name is claiming to know who you are.
//! When the name is not yours, the claim is false on its face — and
//! it says exactly how the address was obtained: from a list where
//! somebody else's name sat beside it. No legitimate sender does
//! this, because a legitimate sender got your address from you.
//!
//! It is also the rare signal a reader can check instantly. *This is
//! addressed to 兰静思. You are 李好.*
//!
//! # Measured
//!
//! Over 36,323 production messages, 357 subjects open with a name and
//! a comma. **201 of them are LinkedIn**, greeting 李好; the rest are
//! Riot Games greeting 召喚師, Duolingo, Google Gemini greeting tora
//! — every one using a name the reader actually goes by.
//!
//! Greeting a name the reader does not go by: **150 messages, and
//! every one of them the same campaign** — five registered domains
//! rotating under one `Reply-To`, an unsubscribe link that is not a
//! `List-Unsubscribe` header, and a volume that goes up when it is
//! followed.
//!
//! # What it needs from the host
//!
//! The names the reader goes by. There is no way to answer "is this
//! name yours" without them, and a deployment that has not said
//! cannot use this check — the same shape as `claims_our_name` and
//! its org names, and it fails the same way: **silently off**, so
//! the caller is expected to say when the list is empty.

/// Openings that are a greeting rather than a name.
///
/// Duolingo writes `哈喽，是多儿。还想学日语吗？` — two Han
/// characters and a comma, and not a name at all. A test caught it
/// on the first run, which is what this list is for.
const NOT_NAMES: &[&str] = &[
    "哈喽", "你好", "您好", "大家好", "亲爱的", "親愛的", "尊敬的",
    "各位", "同学", "同學", "朋友", "老师", "老師", "先生", "女士",
    "早上好", "下午好", "晚上好", "恭喜", "注意", "提醒", "通知",
];

/// The name a subject opens with, if it opens with one.
///
/// `兰静思，您命中……` → `兰静思`. Recognises the CJK form — two to
/// four Han characters followed by a comma, full-width or ASCII —
/// which is what the corpus contains. A Latin-script greeting is a
/// different shape and is not attempted here rather than guessed at.
#[must_use]
pub fn greeted_name(subject: &str) -> Option<&str> {
    let s = subject.trim_start();
    let mut end = 0;
    let mut chars = 0;
    for c in s.chars() {
        if c == ',' || c == '，' {
            if !(2..=4).contains(&chars) {
                return None;
            }
            let word = &s[..end];
            return (!NOT_NAMES.contains(&word)).then_some(word);
        }
        if !('\u{4e00}'..='\u{9fff}').contains(&c) {
            return None;
        }
        chars += 1;
        end += c.len_utf8();
        if chars > 4 {
            return None;
        }
    }
    None
}

/// The greeted name, when the `To:` header does not carry it.
///
/// `to_display` is this message's own decoded `To:` display name —
/// the sender's own claim about who they are writing to. A subject
/// that greets 兰静思 while the `To:` line says nothing of the sort
/// contradicts itself, and needs no list of the reader's names to
/// say so. That matters: a list of names can never be complete, and
/// an incomplete one convicts the 189 LinkedIn messages that greet
/// 李好 — a name production's account row does not contain.
///
/// Empty `to_display` still counts as *not carrying the name*, which
/// is why this is suspicion rather than grounds for hiding mail.
#[must_use]
pub fn greets_someone_else<'a>(subject: &'a str, to_display: &str) -> Option<&'a str> {
    let name = greeted_name(subject)?;
    (!to_display.contains(name)).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The campaign, in its own words. Its `To:` line is the bare
    /// address — it greets a name it never claims to be writing to.
    #[test]
    fn a_name_the_to_line_does_not_carry_is_caught() {
        for subject in [
            "兰静思，恭喜！你的中年翻身仗就要打响了！抓住这次大运就能改命",
            "兰静思，您命中天乙贵人坐配偶，命定姻缘引财神入家门",
            "兰静思,您2026喜逢财星苏醒、转入好运，快请本命佛相助",
        ] {
            assert_eq!(greets_someone_else(subject, "lihao@golia.jp"), Some("兰静思"), "{subject}");
        }
    }

    /// LinkedIn greets 李好 and addresses 李好. 189 of these, and no
    /// list of the reader's names was needed to spare them.
    #[test]
    fn being_greeted_by_the_name_on_the_to_line_is_ordinary_mail() {
        assert_eq!(
            greets_someone_else("李好，添加Mikio Yamaguchi", "李好 <lihao@golia.jp>"),
            None
        );
    }

    /// Duolingo opens `哈喽，是多儿。` — two Han characters and a
    /// comma, and not a name. A test caught this on the first run.
    #[test]
    fn an_opening_greeting_is_not_a_name() {
        assert_eq!(greeted_name("哈喽，是多儿。还想学日语吗？"), None);
        assert_eq!(greeted_name("你好，欢迎使用 Gemini"), None);
        assert_eq!(greeted_name("恭喜，您中奖了"), None);
    }

    #[test]
    fn a_subject_without_a_greeting_is_not_a_claim() {
        for subject in ["【Amazon】配送状況のお知らせ", "Re: 見積書の件", "", "，leading comma", "这是一个很长的中文标题没有逗号"] {
            assert_eq!(greeted_name(subject), None, "{subject}");
        }
    }
}
