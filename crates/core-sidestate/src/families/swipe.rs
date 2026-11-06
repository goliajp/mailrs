//! What a swipe on a conversation row does, per user, read by the web and
//! both phone apps so the gesture is the same on every device.
//!
//! ```text
//! swipe:{user}   JSON {left, right}
//! ```
//!
//! The choices and the defaults are also listed in the shared action table
//! (`web/src/lib/actions.json`, `swipe`); `scripts/check-action-parity.sh`
//! compares the two.

use serde::{Deserialize, Serialize};

/// One direction's action. Toggling read is one choice: the row says which
/// way it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SwipeAction {
    Archive,
    Trash,
    Read,
    None,
}

/// Both directions. Right is a finger moving left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Swipe {
    pub left: SwipeAction,
    pub right: SwipeAction,
}

impl Default for Swipe {
    fn default() -> Self {
        Swipe {
            left: SwipeAction::Trash,
            right: SwipeAction::Archive,
        }
    }
}

/// Key of `user`'s swipe setting.
#[must_use]
pub fn key(user: &str) -> String {
    format!("swipe:{}", user.to_lowercase())
}

/// `user`'s swipe setting; the default when never set.
///
/// # Errors
/// The store could not be read.
pub fn get(conn: &mut kevy_client::Connection, user: &str) -> std::io::Result<Swipe> {
    let Some(bytes) = conn.get(key(user).as_bytes())? else {
        return Ok(Swipe::default());
    };
    Ok(serde_json::from_slice(&bytes).unwrap_or_default())
}

/// Store `user`'s swipe setting.
///
/// # Errors
/// The store could not be written.
pub fn set(conn: &mut kevy_client::Connection, user: &str, s: Swipe) -> std::io::Result<Swipe> {
    let bytes = serde_json::to_vec(&s).map_err(std::io::Error::other)?;
    conn.set(key(user).as_bytes(), &bytes)?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_set_is_right_archive_left_trash() {
        let mut c = kevy_client::Connection::connect("mem://swipe-default").unwrap();
        assert_eq!(
            get(&mut c, "u@x.test").unwrap(),
            Swipe {
                left: SwipeAction::Trash,
                right: SwipeAction::Archive
            }
        );
    }

    #[test]
    fn a_saved_setting_is_read_back_for_any_case_of_the_address() {
        let mut c = kevy_client::Connection::connect("mem://swipe-set").unwrap();
        let mine = Swipe {
            left: SwipeAction::None,
            right: SwipeAction::Read,
        };
        set(&mut c, "U@x.test", mine).unwrap();
        assert_eq!(get(&mut c, "u@x.test").unwrap(), mine);
        assert_eq!(get(&mut c, "other@x.test").unwrap(), Swipe::default());
    }

    #[test]
    fn the_wire_names_are_lowercase_and_an_unknown_one_is_refused() {
        let s: Swipe = serde_json::from_str(r#"{"left":"none","right":"read"}"#).unwrap();
        assert_eq!((s.left, s.right), (SwipeAction::None, SwipeAction::Read));
        assert_eq!(
            serde_json::to_string(&Swipe::default()).unwrap(),
            r#"{"left":"trash","right":"archive"}"#
        );
        assert!(serde_json::from_str::<Swipe>(r#"{"left":"delete","right":"read"}"#).is_err());
    }
}
