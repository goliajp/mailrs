//! What a search asks of a store: which conversations a word matches, and
//! the rows and facts the shared route then judges them by.

use std::collections::{HashMap, HashSet};
use std::future::Future;

use mailrs_core_api::method::conversation::SearchConversationsRequest;
use mailrs_core_api::types::ConversationSummaryWire;
use mailrs_mailbox::query::MessageFacts;
use mailrs_mailbox::search::Stage;

use super::StoreResult;

pub trait SearchStore: Send + Sync + 'static {
    /// `user`'s conversations `word` is a word of, each at the best stage
    /// it is found at — subject, participants or preview; a body; an
    /// attachment's text — at most `want` of each stage. `word` is one of
    /// [`mailrs_mailbox::search::words`], and the store's words are cut
    /// the same way.
    fn word_hits(
        &self,
        user: &str,
        word: &str,
        want: usize,
    ) -> impl Future<Output = StoreResult<Vec<(String, Stage)>>> + Send;

    /// `user`'s conversations whose subject, participants or preview
    /// contain `needle` (lowercase, two characters or more) anywhere,
    /// newest first, at most `want`, leaving out those in `skip`.
    fn substring_hits(
        &self,
        user: &str,
        needle: &str,
        skip: &HashSet<String>,
        want: usize,
    ) -> impl Future<Output = StoreResult<Vec<String>>> + Send;

    /// The conversations of the list `req` was made from, for a list read
    /// off an index of its own (see
    /// [`SearchConversationsRequest::reads_a_list_of_its_own`]), read whole
    /// up to [`mailrs_core_api::method::conversation::LIST_SCOPE_CAP`].
    fn list_ids(
        &self,
        user: &str,
        req: &SearchConversationsRequest,
    ) -> impl Future<Output = StoreResult<HashSet<String>>> + Send;

    /// Every conversation of `user`'s, for a search of operators alone.
    fn all_ids(&self, user: &str) -> impl Future<Output = StoreResult<Vec<String>>> + Send;

    /// `user`'s rows of these conversations, as their lists show them;
    /// one that is not `user`'s is left out.
    fn rows(
        &self,
        user: &str,
        ids: &[String],
    ) -> impl Future<Output = StoreResult<Vec<ConversationSummaryWire>>> + Send;

    /// `user`'s copy of each message of a conversation, for the operators
    /// that ask about messages.
    fn message_facts(
        &self,
        user: &str,
        thread_id: &str,
    ) -> impl Future<Output = StoreResult<Vec<MessageFacts>>> + Send;

    /// The names of the labels on each of `user`'s conversations.
    fn label_names(
        &self,
        user: &str,
    ) -> impl Future<Output = StoreResult<HashMap<String, Vec<String>>>> + Send;
}
