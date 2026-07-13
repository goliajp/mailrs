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
}
