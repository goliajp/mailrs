//! `find_by_uid` + `find_conflicts` + `expand_rrule_utc` (recurrence expansion).

use crate::pg::BackendPool;
use chrono::{DateTime, Datelike, NaiveDateTime, Timelike, Utc};

use super::{CalendarEventRow, EventRowFull, EventRowSqlx};

pub async fn find_conflicts(
    pool: &BackendPool,
    calendar_id: i64,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    exclude_uid: Option<&str>,
) -> Result<Vec<CalendarEventRow>, sqlx::Error> {
    let exclude = exclude_uid.unwrap_or("").to_string();

    // Two-branch candidate fetch:
    //   - static event whose static interval intersects the window
    //   - any recurring series whose dtstart precedes the window end
    //     (we'll RRULE-expand it in Rust to check actual occurrences)
    let candidates: Vec<EventRowFull> = sqlx::query_as::<_, EventRowFull>(
        "SELECT id, calendar_id, uid, etag, summary, dtstart, dtend,
                organizer, status, sequence, method, rrule
         FROM calendar_events
         WHERE calendar_id = $1
           AND (status IS DISTINCT FROM 'CANCELLED')
           AND dtstart IS NOT NULL
           AND ($4 = '' OR uid != $4)
           AND (
                (rrule IS NULL AND dtstart < $3 AND COALESCE(dtend, dtstart) > $2)
             OR (rrule IS NOT NULL AND dtstart < $3)
           )
         ORDER BY dtstart ASC
         LIMIT 200",
    )
    .bind(calendar_id)
    .bind(start)
    .bind(end)
    .bind(exclude)
    .fetch_all(pool)
    .await?;

    let mut conflicts = Vec::with_capacity(candidates.len().min(50));
    for row in candidates {
        let dtstart_orig = match row.dtstart {
            Some(d) => d,
            None => continue,
        };
        let duration = row
            .dtend
            .map(|e| e.signed_duration_since(dtstart_orig))
            .unwrap_or_else(chrono::Duration::zero);

        match &row.rrule {
            Some(rrule_str) if !rrule_str.is_empty() => {
                // Expand and find the first occurrence inside [start, end).
                let occs = expand_rrule_utc(rrule_str, dtstart_orig, start, end);
                for occ in occs {
                    let occ_end = occ + duration;
                    if occ < end && occ_end > start {
                        conflicts.push(CalendarEventRow {
                            id: row.id,
                            calendar_id: row.calendar_id,
                            uid: row.uid.clone(),
                            etag: row.etag.clone(),
                            summary: row.summary.clone(),
                            dtstart: Some(occ),
                            dtend: Some(occ_end),
                            organizer: row.organizer.clone(),
                            status: row.status.clone(),
                            sequence: row.sequence,
                            method: row.method.clone(),
                        });
                        break;
                    }
                }
            }
            _ => {
                conflicts.push(CalendarEventRow {
                    id: row.id,
                    calendar_id: row.calendar_id,
                    uid: row.uid,
                    etag: row.etag,
                    summary: row.summary,
                    dtstart: row.dtstart,
                    dtend: row.dtend,
                    organizer: row.organizer,
                    status: row.status,
                    sequence: row.sequence,
                    method: row.method,
                });
            }
        }

        if conflicts.len() >= 50 {
            break;
        }
    }

    conflicts.sort_by_key(|c| c.dtstart);
    Ok(conflicts)
}

/// Run an RRULE string + DTSTART through the `rrule` crate, returning the
/// occurrences that fall inside [range_start, range_end] in UTC. Failures
/// (parse errors, unsupported rules) silently return an empty vec — the
/// caller falls back to the static-row case which is still a valid result.
fn expand_rrule_utc(
    rrule_str: &str,
    dtstart: DateTime<Utc>,
    range_start: DateTime<Utc>,
    range_end: DateTime<Utc>,
) -> Vec<DateTime<Utc>> {
    use rrule::{RRuleSet, Tz as RTz};

    let dtstart_iso = dtstart.format("%Y%m%dT%H%M%SZ").to_string();
    let source = format!("DTSTART:{dtstart_iso}\nRRULE:{rrule_str}");
    let Ok(set): Result<RRuleSet, _> = source.parse() else {
        return Vec::new();
    };

    use chrono::TimeZone;
    let after = match RTz::UTC
        .with_ymd_and_hms(
            range_start.year(),
            range_start.month(),
            range_start.day(),
            range_start.hour(),
            range_start.minute(),
            range_start.second(),
        )
        .single()
    {
        Some(d) => d,
        None => return Vec::new(),
    };
    let before = match RTz::UTC
        .with_ymd_and_hms(
            range_end.year(),
            range_end.month(),
            range_end.day(),
            range_end.hour(),
            range_end.minute(),
            range_end.second(),
        )
        .single()
    {
        Some(d) => d,
        None => return Vec::new(),
    };

    set.after(after)
        .before(before)
        .all(200)
        .dates
        .into_iter()
        .map(|d| d.with_timezone(&Utc))
        .collect()
}

/// Lookup a single event by (calendar_id, uid). Returns None when absent.
pub async fn find_by_uid(
    pool: &BackendPool,
    calendar_id: i64,
    uid: &str,
) -> Result<Option<CalendarEventRow>, sqlx::Error> {
    let row: Option<EventRowSqlx> = sqlx::query_as(
        "SELECT id, calendar_id, uid, etag, summary, dtstart, dtend,
                organizer, status, sequence, method
         FROM calendar_events
         WHERE calendar_id = $1 AND uid = $2",
    )
    .bind(calendar_id)
    .bind(uid)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(Into::into))
}
