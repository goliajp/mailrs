//! `CalendarStore` impl for `DavAdapter`.

use async_trait::async_trait;

use mailrs_dav::store::{CalendarStore, StoreError};
use mailrs_dav::types::{Calendar, Event, PutResult};

use super::{DavAdapter, to_store_err};

#[async_trait]
impl CalendarStore for DavAdapter {
    async fn list_calendars(&self, user: &str) -> Result<Vec<Calendar>, StoreError> {
        let rows = sqlx::query_as::<_, (i64, String, String, String)>(
            "SELECT id, name, color, description FROM calendars \
             WHERE account_address = $1 ORDER BY name",
        )
        .bind(user)
        .fetch_all(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(rows
            .into_iter()
            .map(|(id, name, color, description)| Calendar {
                id,
                name,
                color,
                description,
            })
            .collect())
    }

    async fn get_calendar(
        &self,
        user: &str,
        calendar_name: &str,
    ) -> Result<Option<Calendar>, StoreError> {
        let row = sqlx::query_as::<_, (i64, String, String, String)>(
            "SELECT id, name, color, description FROM calendars \
             WHERE account_address = $1 AND name = $2",
        )
        .bind(user)
        .bind(calendar_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(row.map(|(id, name, color, description)| Calendar {
            id,
            name,
            color,
            description,
        }))
    }

    async fn list_events(&self, calendar_id: i64) -> Result<Vec<Event>, StoreError> {
        let rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT uid, etag, icalendar FROM calendar_events WHERE calendar_id = $1",
        )
        .bind(calendar_id)
        .fetch_all(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(rows
            .into_iter()
            .map(|(uid, etag, icalendar)| Event {
                uid,
                etag,
                icalendar,
                summary: String::new(),
                dtstart: None,
                dtend: None,
            })
            .collect())
    }

    async fn get_event(&self, calendar_id: i64, uid: &str) -> Result<Option<Event>, StoreError> {
        let row = sqlx::query_as::<_, (String, String)>(
            "SELECT etag, icalendar FROM calendar_events \
             WHERE calendar_id = $1 AND uid = $2",
        )
        .bind(calendar_id)
        .bind(uid)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(row.map(|(etag, icalendar)| Event {
            uid: uid.to_string(),
            etag,
            icalendar,
            summary: String::new(),
            dtstart: None,
            dtend: None,
        }))
    }

    async fn event_etag(&self, calendar_id: i64, uid: &str) -> Result<Option<String>, StoreError> {
        let etag: Option<String> = sqlx::query_scalar(
            "SELECT etag FROM calendar_events WHERE calendar_id = $1 AND uid = $2",
        )
        .bind(calendar_id)
        .bind(uid)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(etag)
    }

    async fn put_event(
        &self,
        calendar_id: i64,
        uid: &str,
        icalendar: &str,
        etag: &str,
    ) -> Result<PutResult, StoreError> {
        let existed: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM calendar_events WHERE calendar_id = $1 AND uid = $2)",
        )
        .bind(calendar_id)
        .bind(uid)
        .fetch_one(&self.pool)
        .await
        .map_err(to_store_err)?;

        // Prefer the structured iTIP-aware path (MRS-3): parse via `mailrs_ical`,
        // project all RFC 5545 / 5546 fields. Falls back to the legacy minimal
        // path for non-VEVENT objects or parser failure — those still write
        // raw + summary so the CalDAV GET round-trip works.
        let parsed = if icalendar.contains("BEGIN:VEVENT") {
            mailrs_ical::parse_invite(icalendar.as_bytes()).ok()
        } else {
            None
        };

        if let Some(ref parsed) = parsed {
            crate::calendar::upsert_from_parsed_invite(
                &self.pool,
                calendar_id,
                uid,
                parsed,
                icalendar,
                etag,
            )
            .await
            .map_err(to_store_err)?;
        } else {
            let summary = mailrs_dav::parse::extract_ical_field(icalendar, "SUMMARY");
            let dtstart = mailrs_dav::parse::extract_ical_datetime(icalendar, "DTSTART");
            let dtend = mailrs_dav::parse::extract_ical_datetime(icalendar, "DTEND");
            sqlx::query(
                "INSERT INTO calendar_events (calendar_id, uid, etag, icalendar, summary, dtstart, dtend)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT (calendar_id, uid)
                 WHERE recurrence_id IS NULL
                 DO UPDATE SET etag = $3, icalendar = $4, summary = $5, dtstart = $6, dtend = $7, updated_at = now()",
            )
            .bind(calendar_id)
            .bind(uid)
            .bind(etag)
            .bind(icalendar)
            .bind(&summary)
            .bind(dtstart)
            .bind(dtend)
            .execute(&self.pool)
            .await
            .map_err(to_store_err)?;
        }

        Ok(PutResult {
            created: !existed,
            etag: etag.to_string(),
        })
    }

    async fn delete_event(&self, calendar_id: i64, uid: &str) -> Result<bool, StoreError> {
        let res = sqlx::query("DELETE FROM calendar_events WHERE calendar_id = $1 AND uid = $2")
            .bind(calendar_id)
            .bind(uid)
            .execute(&self.pool)
            .await
            .map_err(to_store_err)?;
        Ok(res.rows_affected() > 0)
    }

    async fn ensure_default_calendar(&self, user: &str) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO calendars (account_address, name) VALUES ($1, 'Default') ON CONFLICT DO NOTHING",
        )
        .bind(user)
        .execute(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(())
    }
}
