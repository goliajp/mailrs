//! Graham-Robinson naive-Bayes spam classification with Fisher
//! chi-square combining.
//!
//! Pure math — the caller supplies per-token `(spam, ham)` message
//! counts and the corpus totals; this module produces a single spam
//! probability in `[0, 1]`, or `None` when the corpus is too small to
//! trust (the cold-start gate).

/// Per-token training counts — how many spam / ham messages this token
/// appeared in (Graham counts messages, not occurrences).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TokenCounts {
    pub spam: u32,
    pub ham: u32,
}

/// Corpus totals — how many spam / ham messages have been trained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Corpus {
    pub spam_msgs: u32,
    pub ham_msgs: u32,
}

// Cold-start gate: below these the classifier stays silent (returns
// None) so an untrained deployment sees zero effect.
const MIN_TOTAL: u32 = 200;
const MIN_SPAM: u32 = 50;
const MIN_HAM: u32 = 50;

// Robinson smoothing constant `s` and assumed prior `x`.
const ROBINSON_S: f64 = 1.0;
const ROBINSON_X: f64 = 0.5;

// How many most-discriminatory tokens feed the Fisher combiner.
const TOP_N: usize = 15;

/// Classify a tokenized message. `lookup(token)` returns that token's
/// training counts (None = unseen token). Returns the spam probability
/// in `[0, 1]`, or `None` if the corpus hasn't met the cold-start gate.
pub fn classify<F>(tokens: &[String], lookup: F, corpus: &Corpus) -> Option<f64>
where
    F: Fn(&str) -> Option<TokenCounts>,
{
    let total = corpus.spam_msgs + corpus.ham_msgs;
    if total < MIN_TOTAL || corpus.spam_msgs < MIN_SPAM || corpus.ham_msgs < MIN_HAM {
        return None;
    }
    let s_total = corpus.spam_msgs as f64;
    let h_total = corpus.ham_msgs as f64;

    // Per-token Robinson-smoothed spam probability + its discriminatory
    // strength (distance from the neutral 0.5).
    let mut scored: Vec<(f64, f64)> = Vec::new(); // (prob, strength)
    for tok in tokens {
        let Some(c) = lookup(tok) else { continue };
        let n = (c.spam + c.ham) as f64;
        if n == 0.0 {
            continue;
        }
        let b = (c.spam as f64) / s_total; // spam ratio
        let g = (c.ham as f64) / h_total; // ham ratio
        let raw = b / (b + g);
        let p = (ROBINSON_S * ROBINSON_X + n * raw) / (ROBINSON_S + n);
        let p = p.clamp(0.01, 0.99);
        scored.push((p, (p - 0.5).abs()));
    }
    if scored.is_empty() {
        return None;
    }
    // Keep the TOP_N most discriminatory tokens.
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    scored.truncate(TOP_N);

    // Fisher inverse chi-square combining (Robinson's method).
    let n = scored.len() as f64;
    let mut h_ln = 0.0f64; // sum ln(p)   → hamminess side
    let mut s_ln = 0.0f64; // sum ln(1-p) → spamminess side
    for (p, _) in &scored {
        h_ln += p.ln();
        s_ln += (1.0 - p).ln();
    }
    let h = 1.0 - chi2_cdf(-2.0 * h_ln, 2.0 * n);
    let s = 1.0 - chi2_cdf(-2.0 * s_ln, 2.0 * n);
    // Combined indicator in [0,1]; 1 = spam.
    Some((1.0 + h - s) / 2.0)
}

/// A multi-class corpus: N named classes, each with a trained-message
/// total. Used by [`classify_multiclass`] for the v2.9 triage buckets
/// (e.g. classes `["inbox", "notification", "promotion"]`).
#[derive(Debug, Clone, Default)]
pub struct MultiCorpus {
    /// `(class_name, trained_message_count)` for every class, in a
    /// stable order that the per-token `lookup` counts align with.
    pub classes: Vec<(String, u32)>,
}

/// Multi-class classification via **one-vs-rest**: for each class `C`,
/// run the binary [`classify`] treating class `C` as "spam" and the
/// union of all other classes as "ham", then pick the class with the
/// highest resulting probability — provided it clears `min_confidence`.
///
/// `lookup(token)` returns per-class message-counts for that token,
/// aligned with `corpus.classes` (`counts[i]` = # of class-`i` messages
/// containing the token). Returns the winning class index, or `None`
/// when no class clears the cold-start gate (each class still needs
/// `MIN_SPAM` trained messages) or the best confidence is below
/// `min_confidence` — in which case the caller defaults to Inbox. Pure
/// math, reuses all of `classify`'s Robinson/Fisher machinery.
pub fn classify_multiclass<F>(
    tokens: &[String],
    corpus: &MultiCorpus,
    lookup: F,
    min_confidence: f64,
) -> Option<usize>
where
    F: Fn(&str) -> Option<Vec<u32>>,
{
    let n_classes = corpus.classes.len();
    if n_classes < 2 {
        return None;
    }
    let total_msgs: u32 = corpus.classes.iter().map(|(_, c)| *c).sum();

    let mut best: Option<(usize, f64)> = None;
    for i in 0..n_classes {
        let in_class = corpus.classes[i].1;
        let rest = total_msgs.saturating_sub(in_class);
        let ovr_corpus = Corpus {
            spam_msgs: in_class,
            ham_msgs: rest,
        };
        let p = classify(
            tokens,
            |tok| {
                lookup(tok).map(|per_class| {
                    let mine = per_class.get(i).copied().unwrap_or(0);
                    let others: u32 = per_class
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| *j != i)
                        .map(|(_, v)| *v)
                        .sum();
                    TokenCounts {
                        spam: mine,
                        ham: others,
                    }
                })
            },
            &ovr_corpus,
        );
        if let Some(p) = p
            && best.map(|(_, bp)| p > bp).unwrap_or(true)
        {
            best = Some((i, p));
        }
    }
    match best {
        Some((i, p)) if p >= min_confidence => Some(i),
        _ => None,
    }
}

/// Chi-square CDF for even degrees of freedom `df` (df = 2n here).
/// Closed form via the regularized lower incomplete gamma for integer
/// shape — avoids a special-function dependency.
fn chi2_cdf(x: f64, df: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    let k = (df / 2.0) as usize; // integer shape parameter
    // P(shape=k, x/2) via the series that terminates for integer k:
    //   Q = e^{-m} * sum_{i=0}^{k-1} m^i / i!   (upper tail)
    //   CDF = 1 - Q
    let m = x / 2.0;
    let mut term = (-m).exp();
    let mut sum = term;
    for i in 1..k {
        term *= m / (i as f64);
        sum += term;
    }
    (1.0 - sum).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn corpus(spam: u32, ham: u32) -> Corpus {
        Corpus {
            spam_msgs: spam,
            ham_msgs: ham,
        }
    }

    #[test]
    fn cold_start_returns_none() {
        let toks = vec!["viagra".to_string()];
        assert_eq!(classify(&toks, |_| None, &corpus(10, 10)), None);
        assert_eq!(classify(&toks, |_| None, &corpus(60, 10)), None); // ham too few
        assert_eq!(classify(&toks, |_| None, &corpus(10, 60)), None); // spam too few
    }

    #[test]
    fn strong_spam_token_scores_high() {
        let mut counts: HashMap<&str, TokenCounts> = HashMap::new();
        counts.insert("viagra", TokenCounts { spam: 90, ham: 0 });
        counts.insert("meeting", TokenCounts { spam: 1, ham: 80 });
        let corpus = corpus(100, 100);

        let spammy = vec!["viagra".to_string()];
        let p = classify(&spammy, |t| counts.get(t).copied(), &corpus).unwrap();
        assert!(p > 0.85, "spam token should score high, got {p}");

        let hammy = vec!["meeting".to_string()];
        let p2 = classify(&hammy, |t| counts.get(t).copied(), &corpus).unwrap();
        assert!(p2 < 0.15, "ham token should score low, got {p2}");
    }

    #[test]
    fn unseen_tokens_return_none() {
        let corpus = corpus(100, 100);
        let toks = vec!["neverseen".to_string()];
        assert_eq!(classify(&toks, |_| None, &corpus), None);
    }

    #[test]
    fn mixed_message_between_extremes() {
        let mut counts: HashMap<&str, TokenCounts> = HashMap::new();
        counts.insert("viagra", TokenCounts { spam: 80, ham: 2 });
        counts.insert("meeting", TokenCounts { spam: 2, ham: 80 });
        let corpus = corpus(100, 100);
        let toks = vec!["viagra".to_string(), "meeting".to_string()];
        let p = classify(&toks, |t| counts.get(t).copied(), &corpus).unwrap();
        assert!(p > 0.0 && p < 1.0, "mixed message in range, got {p}");
    }

    #[test]
    fn chi2_cdf_monotonic() {
        // Sanity: CDF increases with x for fixed df.
        let a = chi2_cdf(1.0, 4.0);
        let b = chi2_cdf(5.0, 4.0);
        assert!(b > a);
        assert!((0.0..=1.0).contains(&a));
        assert!((0.0..=1.0).contains(&b));
    }

    // ── multi-class (triage) ─────────────────────────────────────────

    fn mc() -> MultiCorpus {
        // classes: 0=inbox, 1=notification, 2=promotion, 100 msgs each.
        MultiCorpus {
            classes: vec![
                ("inbox".into(), 100),
                ("notification".into(), 100),
                ("promotion".into(), 100),
            ],
        }
    }

    #[test]
    fn multiclass_cold_start_returns_none() {
        // A class under MIN_SPAM (50) can't be predicted.
        let small = MultiCorpus {
            classes: vec![("inbox".into(), 10), ("notification".into(), 10)],
        };
        let toks = vec!["hi".to_string()];
        assert_eq!(classify_multiclass(&toks, &small, |_| None, 0.5), None);
    }

    #[test]
    fn multiclass_picks_the_discriminating_class() {
        // "hdr:list-unsub" is overwhelmingly a promotion token; the
        // one-vs-rest classifier should pick class index 2 (promotion).
        let mut counts: HashMap<&str, Vec<u32>> = HashMap::new();
        //                             inbox notif promo
        counts.insert("hdr:list-unsub", vec![1, 5, 90]);
        counts.insert("from:automated", vec![2, 88, 6]);
        let corpus = mc();

        let promo = vec!["hdr:list-unsub".to_string()];
        assert_eq!(
            classify_multiclass(&promo, &corpus, |t| counts.get(t).cloned(), 0.6),
            Some(2)
        );

        let notif = vec!["from:automated".to_string()];
        assert_eq!(
            classify_multiclass(&notif, &corpus, |t| counts.get(t).cloned(), 0.6),
            Some(1)
        );
    }

    #[test]
    fn multiclass_low_confidence_returns_none() {
        // An unseen token → no class clears the confidence gate.
        let corpus = mc();
        let toks = vec!["neverseen".to_string()];
        assert_eq!(classify_multiclass(&toks, &corpus, |_| None, 0.6), None);
    }
}
