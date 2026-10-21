//! Which of a user's conversations a search matches, best match first.
//!
//! Only the thread ids: what each row then looks like, and whether it is in
//! the view the search was made from, is the thread rows' business, the same
//! as for a list.

use std::collections::HashMap;

use crate::pg::PgMailboxStore;

/// At most this many words of a query are searched for.
const MAX_TERMS: usize = 8;

/// The words of a query: runs of letters and digits, lowercased, each once.
/// A query matches a conversation when any of its words does, as in the
/// kevy store's text index, so `partner.test` is `partner` or `test`.
fn terms(query: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in query.split(|c: char| !c.is_alphanumeric()) {
        let word = word.to_lowercase();
        if !word.is_empty() && !out.contains(&word) {
            out.push(word);
        }
    }
    out.truncate(MAX_TERMS);
    out
}

impl PgMailboxStore {
    /// Up to `limit` of `user`'s thread ids whose mail matches any word of
    /// `query`, in the text index or as a substring of the subject, sender,
    /// recipients, body or an attachment's text. Conversations matching more
    /// of the words come first, then by rank within the best word.
    pub async fn search_thread_ids(
        &self,
        user: &str,
        query: &str,
        limit: i64,
    ) -> Result<Vec<String>, sqlx::Error> {
        // (words matched, best position in any one word's ranking)
        let mut found: HashMap<String, (usize, usize)> = HashMap::new();
        for term in terms(query) {
            for (pos, tid) in self
                .search_term(user, &term, limit)
                .await?
                .into_iter()
                .enumerate()
            {
                let e = found.entry(tid).or_insert((0, pos));
                e.0 += 1;
                e.1 = e.1.min(pos);
            }
        }
        let mut ranked: Vec<(String, (usize, usize))> = found.into_iter().collect();
        ranked
            .sort_by(|(ta, (na, pa)), (tb, (nb, pb))| nb.cmp(na).then(pa.cmp(pb)).then(ta.cmp(tb)));
        ranked.truncate(limit as usize);
        Ok(ranked.into_iter().map(|(t, _)| t).collect())
    }

    /// One word's matches, ranked by the text index, then newest first.
    async fn search_term(
        &self,
        user: &str,
        term: &str,
        limit: i64,
    ) -> Result<Vec<String>, sqlx::Error> {
        // One branch per column, joined by UNION, so each can use its own
        // index; an OR-chain across them scans the whole mailbox. Each ILIKE
        // branch repeats its partial index's predicate so the planner can
        // prove the index applies. A term is letters and digits only, so it
        // needs no escaping inside the pattern.
        let sql = "WITH matched AS (
               SELECT id FROM messages WHERE search_vector @@ plainto_tsquery('simple', $2)
               UNION SELECT id FROM messages WHERE subject IS NOT NULL AND subject != '' AND subject ILIKE $3
               UNION SELECT id FROM messages WHERE sender IS NOT NULL AND sender != '' AND sender ILIKE $3
               UNION SELECT id FROM messages WHERE recipients IS NOT NULL AND recipients != '' AND recipients ILIKE $3
               UNION SELECT id FROM messages WHERE text_body IS NOT NULL AND text_body != '' AND text_body ILIKE $3
               UNION SELECT id FROM messages WHERE clean_text IS NOT NULL AND clean_text != '' AND clean_text ILIKE $3
               UNION SELECT message_id FROM attachment_content WHERE extracted_text ILIKE $3
             )
             SELECT m.thread_id
               FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
              WHERE mb.user_address = $1 AND m.thread_id != ''
                AND m.id IN (SELECT id FROM matched)
              GROUP BY m.thread_id
              ORDER BY MAX(CASE WHEN m.search_vector @@ plainto_tsquery('simple', $2)
                                THEN ts_rank(m.search_vector, plainto_tsquery('simple', $2))
                                ELSE 0 END) DESC,
                       MAX(m.internal_date) DESC, m.thread_id
              LIMIT $4";
        let rows: Vec<(String,)> = sqlx::query_as(sql)
            .bind(user)
            .bind(term)
            .bind(format!("%{term}%"))
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|(t,)| t).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::terms;

    #[test]
    fn a_query_is_its_words() {
        assert_eq!(terms("friend@Partner.test"), ["friend", "partner", "test"]);
        assert_eq!(terms("  test, TEST "), ["test"]);
        assert_eq!(terms("请款 50%"), ["请款", "50"]);
        assert!(terms("...").is_empty());
    }

    #[test]
    fn a_long_query_is_capped() {
        let q = (0..20)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(terms(&q).len(), super::MAX_TERMS);
    }
}
