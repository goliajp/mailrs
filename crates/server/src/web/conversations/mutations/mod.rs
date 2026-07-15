//! Mutation handlers for the conversations API.

mod actions;
mod flags;
mod state;

pub(crate) use actions::{record_feedback, toggle_reaction};
pub(crate) use flags::{pin_thread, star_thread, unpin_thread, unstar_thread};
pub(crate) use state::{
    archive_thread, delete_thread, mark_junk, mark_not_junk, mark_notification, mark_promotion,
    mark_thread_read, mark_thread_unread, move_to_inbox, snooze_thread, unarchive_thread,
    unsnooze_thread,
};
