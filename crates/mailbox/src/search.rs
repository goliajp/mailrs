//! What a search's words are and how they combine, for every store.
//!
//! One tokenizer for the query and for what is indexed, in both stores, so
//! a word one store finds the other finds too; and one rule for a query of
//! several words: every word must match.

use std::collections::HashMap;

/// At most this many words of a query are searched for.
pub const MAX_WORDS: usize = 8;

/// Where a word matched a conversation, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    /// A word of its subject, its participants or its preview.
    Header,
    /// A word of the body of one of its messages.
    Body,
    /// A word of the text of one of its attachments.
    Attachment,
    /// Inside its subject, participants or preview, not as a whole word.
    Substring,
}

/// The words of `text`, in order, each once, at most [`MAX_WORDS`]: Latin
/// letters and digits split on everything else and lowercased, words of
/// one character dropped, CJK as overlapping pairs of characters.
pub fn words(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in kevy_text::tokenize(text.as_bytes()) {
        let word = String::from_utf8_lossy(&token).into_owned();
        if out.contains(&word) {
            continue;
        }
        out.push(word);
        if out.len() == MAX_WORDS {
            break;
        }
    }
    out
}

/// Every word of `text`, repeats kept, joined by spaces: what a store
/// whose own tokenizer splits on spaces alone is given to index, so the
/// words it ends up with are exactly the ones [`words`] makes.
pub fn indexed_words(text: &str) -> String {
    let tokens = kevy_text::tokenize(text.as_bytes());
    let mut out = String::new();
    for token in tokens {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&String::from_utf8_lossy(&token));
    }
    out
}

/// The conversations every word matched, given each word's matches, with
/// the worst of the stages each word matched it at best; best first, then
/// by id. A conversation one word missed is not among them.
pub fn every_word(per_word: &[Vec<(String, Stage)>]) -> Vec<(String, Stage)> {
    let Some((first, rest)) = per_word.split_first() else {
        return Vec::new();
    };
    let best = |hits: &[(String, Stage)]| {
        let mut by_id: HashMap<String, Stage> = HashMap::new();
        for (id, stage) in hits {
            let e = by_id.entry(id.clone()).or_insert(*stage);
            *e = (*e).min(*stage);
        }
        by_id
    };
    let mut kept = best(first);
    for hits in rest {
        let these = best(hits);
        kept.retain(|id, stage| match these.get(id) {
            Some(s) => {
                *stage = (*stage).max(*s);
                true
            }
            None => false,
        });
    }
    let mut out: Vec<(String, Stage)> = kept.into_iter().collect();
    out.sort_by(|(a, sa), (b, sb)| sa.cmp(sb).then_with(|| a.cmp(b)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dotted_address_is_its_words() {
        assert_eq!(words("outside.example"), ["outside", "example"]);
        assert_eq!(words("Bob@Example.TEST"), ["bob", "example", "test"]);
    }

    #[test]
    fn cjk_is_pairs_and_a_lone_character_stays() {
        assert_eq!(words("請求書"), ["請求", "求書"]);
        assert_eq!(words("犬"), ["犬"]);
    }

    #[test]
    fn words_are_kept_once_and_capped() {
        assert_eq!(words("a b ab AB ab"), ["ab"]);
        let many = "w1 w2 w3 w4 w5 w6 w7 w8 w9";
        assert_eq!(words(many).len(), MAX_WORDS);
    }

    #[test]
    fn the_indexed_text_has_the_same_words() {
        let text = "Re: Invoice #42 from billing@shop.example — 請求書";
        assert_eq!(
            indexed_words(text),
            "re invoice 42 from billing shop example 請求 求書"
        );
    }

    fn hits(list: &[(&str, Stage)]) -> Vec<(String, Stage)> {
        list.iter().map(|(id, s)| (id.to_string(), *s)).collect()
    }

    #[test]
    fn every_word_must_match() {
        let outside = hits(&[("t1", Stage::Body)]);
        let example = hits(&[("t1", Stage::Header), ("t2", Stage::Header)]);
        assert_eq!(
            every_word(&[outside, example]),
            hits(&[("t1", Stage::Body)])
        );
    }

    #[test]
    fn a_conversation_ranks_by_its_weakest_word() {
        let first = hits(&[
            ("a", Stage::Header),
            ("a", Stage::Substring),
            ("b", Stage::Header),
        ]);
        let second = hits(&[("a", Stage::Attachment), ("b", Stage::Header)]);
        assert_eq!(
            every_word(&[first, second]),
            hits(&[("b", Stage::Header), ("a", Stage::Attachment)])
        );
    }

    #[test]
    fn no_words_match_nothing() {
        assert!(every_word(&[]).is_empty());
        assert!(every_word(&[hits(&[("a", Stage::Header)]), Vec::new()]).is_empty());
    }
}
