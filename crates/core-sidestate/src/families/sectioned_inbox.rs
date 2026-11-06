//! Whether the Inbox is split into sections (all, to reply, waiting, new
//! senders), per user, read by the web and both phone apps so a choice made
//! on one follows to the others.
//!
//! ```text
//! sectioned_inbox:{user}   JSON {enabled}
//! ```

use serde::{Deserialize, Serialize};

/// On unless the user turned it off, as Gmail's default inbox shows its tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionedInbox {
    pub enabled: bool,
}

impl Default for SectionedInbox {
    fn default() -> Self {
        SectionedInbox { enabled: true }
    }
}

/// Key of `user`'s sectioned inbox setting.
#[must_use]
pub fn key(user: &str) -> String {
    format!("sectioned_inbox:{}", user.to_lowercase())
}

/// `user`'s setting; the default when never set.
///
/// # Errors
/// The store could not be read, or holds something that is not the setting.
pub fn get(conn: &mut kevy_client::Connection, user: &str) -> std::io::Result<SectionedInbox> {
    let Some(bytes) = conn.get(key(user).as_bytes())? else {
        return Ok(SectionedInbox::default());
    };
    serde_json::from_slice(&bytes).map_err(std::io::Error::other)
}

/// Store `user`'s setting.
///
/// # Errors
/// The store could not be written.
pub fn set(
    conn: &mut kevy_client::Connection,
    user: &str,
    s: SectionedInbox,
) -> std::io::Result<SectionedInbox> {
    let bytes = serde_json::to_vec(&s).map_err(std::io::Error::other)?;
    conn.set(key(user).as_bytes(), &bytes)?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_set_is_on() {
        let mut c = kevy_client::Connection::connect("mem://sectioned-default").unwrap();
        assert_eq!(
            get(&mut c, "u@x.test").unwrap(),
            SectionedInbox { enabled: true }
        );
    }

    #[test]
    fn turned_off_is_read_back_for_any_case_of_the_address() {
        let mut c = kevy_client::Connection::connect("mem://sectioned-set").unwrap();
        set(&mut c, "U@x.test", SectionedInbox { enabled: false }).unwrap();
        assert_eq!(
            get(&mut c, "u@x.test").unwrap(),
            SectionedInbox { enabled: false }
        );
        assert_eq!(
            get(&mut c, "other@x.test").unwrap(),
            SectionedInbox::default()
        );
    }

    #[test]
    fn the_wire_shape_is_a_boolean_named_enabled() {
        assert_eq!(
            serde_json::to_string(&SectionedInbox::default()).unwrap(),
            r#"{"enabled":true}"#
        );
        assert!(serde_json::from_str::<SectionedInbox>(r#"{"enabled":"no"}"#).is_err());
        assert!(serde_json::from_str::<SectionedInbox>("{}").is_err());
    }
}
