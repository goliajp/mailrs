//! A remote image of the signed-in user's mail, as fastcore prefetched it.

use axum::extract::{Extension, Path};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use mailrs_core_sidestate::image_cache;

use crate::handlers::conversations::AuthedUser;

/// GET /api/mail/image/{name} — `name` is the sha256 of the image's URL as
/// the message wrote it. Looked up only in the caller's own directory, so
/// a hash says nothing about anyone else's mail. 404 when it was not
/// fetched (refused, over budget, Junk, or not yet).
pub async fn get_image(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(name): Path<String>,
) -> Response {
    if name.len() != 64 || !name.bytes().all(|b| b.is_ascii_hexdigit()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let path = image_cache::user_dir(&image_cache::root(), &user).join(name.to_ascii_lowercase());
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // the cache keeps what was read lately; the read is its clock
    if let Ok(f) = std::fs::File::options().write(true).open(&path) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
    let kind = match bytes.starts_with(b"\x89PNG") {
        true => "image/png",
        false => "image/jpeg",
    };
    (
        [
            (header::CONTENT_TYPE, kind),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (header::CACHE_CONTROL, "private, max-age=86400"),
        ],
        bytes,
    )
        .into_response()
}

/// POST /api/conversations/{thread_id}/images — fetch the thread's remote
/// images now, for mail that arrived before images were fetched on
/// arrival; the page asks again for the ones it could not load. 409 for a
/// thread in Junk or held, unless `?anyway=true`.
/// `?anyway=true`: the reader asked for a thread in Junk or held all the same.
#[derive(serde::Deserialize, Default)]
pub struct Anyway {
    #[serde(default)]
    anyway: bool,
}

pub async fn prefetch_thread(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<Anyway>,
) -> Response {
    match state
        .core
        .prefetch_thread_images(&user, &thread_id, q.anyway)
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(mailrs_core_api::error::CoreApiError::Conflict(_)) => (
            StatusCode::CONFLICT,
            axum::Json(serde_json::json!({ "error": "images_not_fetched_for_junk" })),
        )
            .into_response(),
        Err(e) => crate::handlers::conversations::map_err(e).into_response(),
    }
}
