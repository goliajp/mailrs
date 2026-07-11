//! Read-only conversations API handlers, split by query type.

mod aggregates;
mod list;
mod thread;

pub(crate) use aggregates::{get_contacts, get_conversation_categories, get_mail_stats};
pub(crate) use list::{batch_conversations, get_conversations};
pub(crate) use thread::fetch_message_reactions;
pub(crate) use thread::{get_thread_messages, get_thread_reactions};
