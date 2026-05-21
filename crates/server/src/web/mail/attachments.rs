//! Attachment download / inline image upload + serve / extracted-content fetch.

use std::sync::Arc;

use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use mail_parser::MimeHeaders;
use serde::Serialize;

use crate::message_util;

use super::{AuthUser, WebState};

// --- attachment content (OCR/PDF text) ---

#[derive(Serialize)]
struct AttachmentContentResponse {
    success: bool,
    extracted_text: Option<String>,
    language: Option<String>,
    // f64 matches the DOUBLE PRECISION column type
    confidence: f64,
    page_count: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

// --- inline image handlers ---

#[derive(Serialize)]
struct InlineUploadResult {
    success: bool,
    id: Option<String>,
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

pub(crate) async fn get_attachment(
    Path((uid, index)): Path<(u32, usize)>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return (
            StatusCode::NOT_FOUND,
            [
                ("content-type", "text/plain".to_string()),
                ("content-disposition", String::new()),
            ],
            Vec::new(),
        );
    };

    let mailboxes = mb_store.list_mailboxes(&user).await.unwrap_or_default();
    for mb in &mailboxes {
        if let Ok(Some(msg)) = mb_store.get_message(mb.id, uid).await {
            let raw = message_util::read_message_raw(&state.maildir_root, &user, &msg.maildir_id);
            if let Some(data) = raw
                && let Some(parsed) = mail_parser::MessageParser::default().parse(&data) {
                    let attachments: Vec<_> = parsed.attachments().collect();
                    if let Some(att) = attachments.get(index) {
                        let filename = att
                            .attachment_name()
                            .or_else(|| att.content_type().and_then(|ct| ct.attribute("name")))
                            .unwrap_or("unnamed")
                            .to_string();
                        let content_type = att
                            .content_type()
                            .map(|ct| {
                                if let Some(sub) = ct.subtype() {
                                    format!("{}/{}", ct.ctype(), sub)
                                } else {
                                    ct.ctype().to_string()
                                }
                            })
                            .unwrap_or_else(|| "application/octet-stream".into());
                        let body = att.contents().to_vec();

                        // use inline for browser-viewable types, attachment for the rest
                        let inline = content_type.starts_with("image/")
                            || content_type.starts_with("text/")
                            || content_type == "application/pdf";
                        let param = message_util::rfc2231_encode_param("filename", &filename);
                        let disposition = if inline {
                            format!("inline; {param}")
                        } else {
                            format!("attachment; {param}")
                        };

                        return (
                            StatusCode::OK,
                            [
                                ("content-type", content_type),
                                ("content-disposition", disposition),
                            ],
                            body,
                        );
                    }
                }
        }
    }

    (
        StatusCode::NOT_FOUND,
        [
            ("content-type", "text/plain".to_string()),
            ("content-disposition", String::new()),
        ],
        b"attachment not found".to_vec(),
    )
}

pub(crate) async fn get_attachment_content(
    Path((uid, index)): Path<(u32, i16)>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return Json(AttachmentContentResponse {
            success: false,
            extracted_text: None,
            language: None,
            confidence: 0.0,
            page_count: None,
            message: Some("database unavailable".into()),
        });
    };

    // resolve message id and attachment content in a single query, avoiding the
    // N+1 pattern of listing all mailboxes then probing each one for the uid
    let row = sqlx::query_as::<_, (String, Option<String>, f64, Option<i16>)>(
        "SELECT COALESCE(ac.extracted_text, ''), ac.language, ac.ocr_confidence, ac.page_count
         FROM attachment_content ac
         JOIN messages m ON ac.message_id = m.id
         JOIN mailboxes mb ON m.mailbox_id = mb.id
         WHERE mb.user_address = $1 AND m.uid = $2 AND ac.attachment_index = $3
         LIMIT 1",
    )
    .bind(&user)
    .bind(uid as i32)
    .bind(index)
    .fetch_optional(pool)
    .await;

    match row {
        Ok(Some((text, language, confidence, page_count))) => Json(AttachmentContentResponse {
            success: true,
            extracted_text: Some(text),
            language,
            confidence,
            page_count,
            message: None,
        }),
        Ok(None) => Json(AttachmentContentResponse {
            success: false,
            extracted_text: None,
            language: None,
            confidence: 0.0,
            page_count: None,
            message: Some("content not yet extracted".into()),
        }),
        Err(_) => Json(AttachmentContentResponse {
            success: false,
            extracted_text: None,
            language: None,
            confidence: 0.0,
            page_count: None,
            message: Some("internal error".into()),
        }),
    }
}

pub(crate) async fn upload_inline_image(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut image_data: Option<Vec<u8>> = None;
    let mut content_type = String::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "image" {
            content_type = field
                .content_type()
                .unwrap_or("application/octet-stream")
                .to_string();
            if let Ok(data) = field.bytes().await {
                image_data = Some(data.to_vec());
            }
        }
    }

    let Some(data) = image_data else {
        return Json(InlineUploadResult {
            success: false,
            id: None,
            url: None,
            message: Some("no image field provided".into()),
        });
    };

    if let Err(e) = crate::inline_image::validate_inline_upload(&data, &content_type) {
        return Json(InlineUploadResult {
            success: false,
            id: None,
            url: None,
            message: Some(e),
        });
    }

    let id = crate::inline_image::generate_inline_id();
    let ext = crate::inline_image::ext_from_content_type(&content_type);
    let path = crate::inline_image::inline_path(&state.maildir_root, &user, &id, ext);

    if let Some(parent) = path.parent()
        && let Err(e) = tokio::fs::create_dir_all(parent).await {
            return Json(InlineUploadResult {
                success: false,
                id: None,
                url: None,
                message: Some(format!("create dir: {e}")),
            });
        }

    if let Err(e) = tokio::fs::write(&path, &data).await {
        return Json(InlineUploadResult {
            success: false,
            id: None,
            url: None,
            message: Some(format!("write file: {e}")),
        });
    }

    let url = format!("/api/mail/inline/{id}");
    Json(InlineUploadResult {
        success: true,
        id: Some(id),
        url: Some(url),
        message: None,
    })
}

pub(crate) async fn serve_inline_image(
    Path(id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    use axum::http::header;

    // validate ID format: only permit strictly safe alphanumeric+underscore IDs
    if !crate::inline_image::is_valid_inline_id(&id) {
        return (StatusCode::BAD_REQUEST, "invalid id").into_response();
    }

    // try each known extension for exact file match (avoids prefix collision)
    let known_exts = ["png", "jpg", "webp", "gif", "tiff", "bmp", "svg", "bin"];
    let mut found = None;
    for ext in &known_exts {
        let path = crate::inline_image::inline_path(&state.maildir_root, &user, &id, ext);
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            found = Some(path);
            break;
        }
    }

    let Some(file_path) = found else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };

    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    let content_type = match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "tiff" => "image/tiff",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    };

    match tokio::fs::read(&file_path).await {
        Ok(data) => {
            let mut resp = (StatusCode::OK, data).into_response();
            let h = resp.headers_mut();
            h.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
            h.insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
            h.insert(header::CONTENT_DISPOSITION, "inline".parse().unwrap());
            resp
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "read error").into_response(),
    }
}
