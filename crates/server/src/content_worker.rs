//! background worker for extracting text content from email attachments.
//!
//! polls messages table for unprocessed attachments and runs OCR / PDF
//! text extraction via content_extract module. results are stored in
//! the attachment_content table for full-text search.

use std::time::Duration;

use sqlx::PgPool;

use crate::message_util;
use mailrs_attachment_extract::{self, ExtractionResult, MAX_EXTRACT_SIZE};

/// interval between worker polls
const POLL_INTERVAL: Duration = Duration::from_secs(30);

/// batch size per poll cycle
const BATCH_SIZE: i64 = 10;

/// default OCR languages
const DEFAULT_OCR_LANGS: &str = "eng+chi_sim+jpn";

pub fn spawn_content_worker(pool: PgPool, maildir_root: String) {
    tokio::spawn(async move {
        content_worker_loop(pool, maildir_root).await;
    });
    tracing::info!(event = "subsystem_started", subsystem = "content_worker");
}

async fn content_worker_loop(pool: PgPool, maildir_root: String) {
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    loop {
        interval.tick().await;
        if let Err(e) = process_batch(&pool, &maildir_root).await {
            tracing::error!(event = "content_worker_batch_failed", error = %e);
        }
    }
}

/// find messages with attachments that haven't been processed yet
async fn process_batch(pool: &PgPool, maildir_root: &str) -> Result<(), String> {
    // find messages that have attachments but no attachment_content rows yet
    let rows = sqlx::query_as::<_, (i64, String, String, String)>(
        "SELECT m.id, m.sender, m.maildir_id, mb.user_address
         FROM messages m
         JOIN mailboxes mb ON m.mailbox_id = mb.id
         WHERE m.size > 0
         AND NOT EXISTS (
             SELECT 1 FROM attachment_content ac WHERE ac.message_id = m.id
         )
         ORDER BY m.id DESC
         LIMIT $1",
    )
    .bind(BATCH_SIZE)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("query unprocessed: {e}"))?;

    for (message_id, _sender, maildir_id, user_address) in rows {
        if let Err(e) =
            process_message(pool, maildir_root, message_id, &maildir_id, &user_address).await
        {
            tracing::warn!(
                event = "content_worker_process_failed",
                message_id,
                error = %e
            );
            // insert a sentinel row so we don't retry forever
            if let Err(sentinel_err) = insert_empty_sentinel(pool, message_id).await {
                tracing::error!(
                    event = "content_worker_sentinel_failed",
                    message_id,
                    error = %sentinel_err
                );
            }
        }
    }

    Ok(())
}

/// process a single message's attachments
async fn process_message(
    pool: &PgPool,
    maildir_root: &str,
    message_id: i64,
    maildir_id: &str,
    user_address: &str,
) -> Result<(), String> {
    let raw = message_util::read_message_raw(maildir_root, user_address, maildir_id)
        .ok_or("raw message not found")?;

    let parsed = mailrs_mime::parse(&raw);
    let attachments: Vec<&mailrs_mime::Part> = parsed.attachments().collect();

    if attachments.is_empty() {
        // no attachments — insert sentinel so we skip this message next time
        insert_empty_sentinel(pool, message_id).await?;
        return Ok(());
    }

    let mut inserted = 0u32;
    for (index, att) in attachments.iter().enumerate() {
        let content_type = att.content_type.mime_type();

        // skip unsupported types early
        let method = mailrs_attachment_extract::extraction_method(&content_type);
        if method == mailrs_attachment_extract::ExtractionMethod::Unsupported {
            continue;
        }

        let data = &att.body;
        if data.len() > MAX_EXTRACT_SIZE {
            continue;
        }

        // run extraction in blocking thread (tesseract is CPU-bound)
        let data_owned = data.clone();
        let ct = content_type.clone();
        let result = tokio::task::spawn_blocking(move || {
            mailrs_attachment_extract::extract_content(&data_owned, &ct, DEFAULT_OCR_LANGS)
        })
        .await
        .map_err(|e| format!("spawn_blocking: {e}"))?;

        match result {
            Ok(extraction) => {
                insert_extraction(pool, message_id, index as i16, &content_type, &extraction)
                    .await?;
                inserted += 1;
            }
            Err(e) => {
                tracing::warn!(
                    event = "content_worker_extract_failed",
                    message_id,
                    attachment_index = index,
                    error = %e
                );
            }
        }
    }

    // if no extractable attachments were found, insert sentinel
    if inserted == 0 {
        insert_empty_sentinel(pool, message_id).await?;
    }

    Ok(())
}

async fn insert_extraction(
    pool: &PgPool,
    message_id: i64,
    attachment_index: i16,
    content_type: &str,
    result: &ExtractionResult,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO attachment_content
         (message_id, attachment_index, content_type, extracted_text, language, ocr_confidence, page_count, metadata)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (message_id, attachment_index) DO NOTHING",
    )
    .bind(message_id)
    .bind(attachment_index)
    .bind(content_type)
    .bind(&result.text)
    .bind(&result.language)
    .bind(result.confidence)
    .bind(result.page_count.map(|p| p as i16))
    .bind(&result.metadata)
    .execute(pool)
    .await
    .map_err(|e| format!("insert extraction: {e}"))?;

    Ok(())
}

/// insert a sentinel row so the worker doesn't re-process this message
async fn insert_empty_sentinel(pool: &PgPool, message_id: i64) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO attachment_content
         (message_id, attachment_index, content_type, extracted_text)
         VALUES ($1, -1, 'none', '')
         ON CONFLICT (message_id, attachment_index) DO NOTHING",
    )
    .bind(message_id)
    .execute(pool)
    .await
    .map_err(|e| format!("insert sentinel: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ocr_langs_includes_english() {
        assert!(DEFAULT_OCR_LANGS.contains("eng"));
    }

    #[test]
    fn default_ocr_langs_includes_chinese() {
        assert!(DEFAULT_OCR_LANGS.contains("chi_sim"));
    }

    #[test]
    fn default_ocr_langs_includes_japanese() {
        assert!(DEFAULT_OCR_LANGS.contains("jpn"));
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn batch_size_reasonable() {
        assert!(BATCH_SIZE > 0 && BATCH_SIZE <= 100);
    }

    #[test]
    fn poll_interval_reasonable() {
        assert!(POLL_INTERVAL.as_secs() >= 10);
        assert!(POLL_INTERVAL.as_secs() <= 300);
    }
}
