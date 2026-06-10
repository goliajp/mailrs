//! `write_to_own_calendar` — upsert accepted/tentative invites
//! into the user's default calendar.

use chrono::{DateTime, Utc};

pub(super) async fn write_to_own_calendar(
    pool: &crate::pg::BackendPool,
    user: &str,
    invite_payload: &serde_json::Value,
    partstat: &str,
    now: DateTime<Utc>,
    recurrence_id: Option<DateTime<Utc>>,
) -> Result<(), sqlx::Error> {
    let uid = invite_payload
        .get("uid")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if uid.is_empty() {
        return Ok(());
    }

    // Ensure the user has a default calendar (matches the CalDAV path).
    sqlx::query(
        "INSERT INTO calendars (account_address, name)
         VALUES ($1, 'Default') ON CONFLICT DO NOTHING",
    )
    .bind(user)
    .execute(pool)
    .await?;

    let cal_id: i64 = sqlx::query_scalar(
        "SELECT id FROM calendars WHERE account_address = $1 ORDER BY id ASC LIMIT 1",
    )
    .bind(user)
    .fetch_one(pool)
    .await?;

    let summary = invite_payload
        .get("summary")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    let dtstart_utc = extract_caldatetime_to_utc(invite_payload.get("dtstart"));
    let dtend_utc = extract_caldatetime_to_utc(invite_payload.get("dtend"));
    let status_str = if partstat == "TENTATIVE" {
        "TENTATIVE"
    } else {
        "CONFIRMED"
    };
    let etag = format!("{:x}", now.timestamp_micros());

    // Reuse the structured upsert path (pre-flight: minimal raw form built
    // fresh; the exact bytes are not what CalDAV clients GET — they GET
    // whatever later round-trip produces. For now keep it minimal +
    // deterministic).
    let raw_min = format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//mailrs//MRS-1 ICS//EN\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nSUMMARY:{summary}\r\nSTATUS:{status_str}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
    );

    let sql = if recurrence_id.is_some() {
        "INSERT INTO calendar_events
            (calendar_id, uid, etag, icalendar, summary, dtstart, dtend, status, last_modified, recurrence_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         ON CONFLICT (calendar_id, uid, recurrence_id) WHERE recurrence_id IS NOT NULL DO UPDATE SET
            etag = EXCLUDED.etag,
            icalendar = EXCLUDED.icalendar,
            summary = EXCLUDED.summary,
            dtstart = EXCLUDED.dtstart,
            dtend = EXCLUDED.dtend,
            status = EXCLUDED.status,
            last_modified = EXCLUDED.last_modified,
            updated_at = now()"
    } else {
        "INSERT INTO calendar_events
            (calendar_id, uid, etag, icalendar, summary, dtstart, dtend, status, last_modified, recurrence_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         ON CONFLICT (calendar_id, uid) WHERE recurrence_id IS NULL DO UPDATE SET
            etag = EXCLUDED.etag,
            icalendar = EXCLUDED.icalendar,
            summary = EXCLUDED.summary,
            dtstart = EXCLUDED.dtstart,
            dtend = EXCLUDED.dtend,
            status = EXCLUDED.status,
            last_modified = EXCLUDED.last_modified,
            updated_at = now()"
    };

    sqlx::query(sql)
        .bind(cal_id)
        .bind(uid)
        .bind(&etag)
        .bind(&raw_min)
        .bind(summary)
        .bind(dtstart_utc)
        .bind(dtend_utc)
        .bind(status_str)
        .bind(now)
        .bind(recurrence_id)
        .execute(pool)
        .await?;

    Ok(())
}

pub(super) fn extract_caldatetime_to_utc(v: Option<&serde_json::Value>) -> Option<DateTime<Utc>> {
    let v = v?;
    let obj = v.as_object()?;
    let (variant, inner) = obj.iter().next()?;
    match variant.as_str() {
        "Utc" => inner.as_str().and_then(|s| s.parse().ok()),
        "Floating" => inner
            .as_str()
            .and_then(|s| s.parse::<chrono::NaiveDateTime>().ok())
            .map(|n| n.and_utc()),
        "Date" => inner
            .as_str()
            .and_then(|s| s.parse::<chrono::NaiveDate>().ok())
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .map(|nd| nd.and_utc()),
        // Zoned: needs tz resolution; skip for v1 (rare for REPLY-side
        // write-back since the user sees the time the organizer chose, not
        // a zone they picked themselves).
        _ => None,
    }
}
