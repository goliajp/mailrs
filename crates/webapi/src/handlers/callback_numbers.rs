//! The phone numbers a sender gave the reader before, in any conversation.
//!
//! - `GET /api/senders/{address}/callback-numbers`
//!
//! The reading pane points out a number in a message that its sender has
//! not given the reader before; this is the history it compares against,
//! the signed-in reader's own and nobody else's. Read only: delivery and
//! the attachment reader write it.

use axum::Json;
use axum::extract::{Extension, Path};
use axum::http::StatusCode;
use mailrs_core_sidestate::families::callback_numbers;

use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

/// One number, and the earliest message that gave it.
#[derive(Debug, serde::Serialize)]
pub struct GivenNumber {
    /// The number's digits.
    pub number: String,
    /// When that message arrived here, unix seconds.
    pub first_seen: i64,
    /// That message's id: a number first given by the message being read
    /// is not one the sender gave before it.
    pub first_message_id: String,
}

#[derive(Debug, serde::Serialize)]
pub struct CallbackNumbers {
    /// The address asked about, as the history keys it.
    pub sender: String,
    /// Earliest first.
    pub numbers: Vec<GivenNumber>,
}

/// GET /api/senders/{address}/callback-numbers
pub async fn list(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<Json<CallbackNumbers>, StatusCode> {
    let sender = mailrs_core_sidestate::families::sender_rules::normalize(&address);
    if sender.is_empty() || !sender.contains('@') {
        return Err(StatusCode::BAD_REQUEST);
    }
    let who = sender.clone();
    let given = with_kevy(move |c| callback_numbers::given(c, &user, &who))?;
    Ok(Json(CallbackNumbers {
        sender,
        numbers: given
            .into_iter()
            .map(|g| GivenNumber {
                number: g.number,
                first_seen: g.first_seen,
                first_message_id: g.first_message_id,
            })
            .collect(),
    }))
}
