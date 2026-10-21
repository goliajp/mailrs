//! Path and method routing over the handlers, for a server that mounts the
//! whole `/dav/` tree on one route.
//!
//! The wrapper authenticates and passes the user; the user segment of every
//! path must name that same user or the request is refused with 403, so a
//! client signed in as one account cannot address another's collections.
//!
//! ```text
//!   /dav/                                     principal
//!   /dav/principals/[{user}/]                 principal
//!   /dav/calendars/{user}/                    calendar home
//!   /dav/calendars/{user}/{calendar}/         calendar
//!   /dav/calendars/{user}/{calendar}/{uid}.ics
//!   /dav/addressbooks/{user}/                 address-book home
//!   /dav/addressbooks/{user}/{book}/          address book
//!   /dav/addressbooks/{user}/{book}/{uid}.vcf
//! ```

use crate::error::DavError;
use crate::parse::{parse_depth, percent_decode};
use crate::store::{AddressBookStore, CalendarStore, into_dav_error};
use crate::xml::{DavResponse, options_response};
use crate::{caldav, carddav, principal};

/// One request, as the dispatcher needs it.
#[derive(Debug, Clone, Copy)]
pub struct DavRequest<'a> {
    /// HTTP method, e.g. `PROPFIND`.
    pub method: &'a str,
    /// The request path, still percent-encoded.
    pub path: &'a str,
    /// `Depth` header.
    pub depth: Option<&'a str>,
    /// `If-Match` header.
    pub if_match: Option<&'a str>,
    /// `If-None-Match` header.
    pub if_none_match: Option<&'a str>,
    /// Request body, already checked to be UTF-8.
    pub body: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
enum Target {
    Principal,
    CalendarHome,
    Calendar(String),
    Event(String, String),
    BookHome,
    Book(String),
    Contact(String, String),
}

fn decode(segment: &str) -> Result<String, DavError> {
    percent_decode(segment).ok_or_else(|| DavError::BadRequest(format!("segment {segment:?}")))
}

fn resource(segment: &str, suffix: &str) -> Result<String, DavError> {
    let name = decode(segment)?;
    match name.strip_suffix(suffix) {
        Some(uid) if !uid.is_empty() => Ok(uid.to_string()),
        _ => Err(DavError::NotFound),
    }
}

fn parse_target(path: &str, user: &str) -> Result<Target, DavError> {
    let rest = path
        .strip_prefix("/dav")
        .filter(|r| r.is_empty() || r.starts_with('/'))
        .ok_or(DavError::NotFound)?;
    let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    let owner_ok = |seg: &str| -> Result<(), DavError> {
        match decode(seg)? == user {
            true => Ok(()),
            false => Err(DavError::Forbidden),
        }
    };
    match segments.as_slice() {
        [] | ["principals"] => Ok(Target::Principal),
        ["principals", u] => owner_ok(u).map(|_| Target::Principal),
        ["calendars", u, tail @ ..] => {
            owner_ok(u)?;
            match tail {
                [] => Ok(Target::CalendarHome),
                [c] => Ok(Target::Calendar(decode(c)?)),
                [c, r] => Ok(Target::Event(decode(c)?, resource(r, ".ics")?)),
                _ => Err(DavError::NotFound),
            }
        }
        ["addressbooks", u, tail @ ..] => {
            owner_ok(u)?;
            match tail {
                [] => Ok(Target::BookHome),
                [b] => Ok(Target::Book(decode(b)?)),
                [b, r] => Ok(Target::Contact(decode(b)?, resource(r, ".vcf")?)),
                _ => Err(DavError::NotFound),
            }
        }
        _ => Err(DavError::NotFound),
    }
}

/// Route `req` to the handler its path and method name.
///
/// `user` is the authenticated account. Errors come back unconverted so
/// the wrapper can log a [`DavError::ServerError`]'s description before
/// answering with [`DavError::to_dav_response`].
pub async fn handle(
    req: &DavRequest<'_>,
    user: &str,
    calendars: &dyn CalendarStore,
    books: &dyn AddressBookStore,
) -> Result<DavResponse, DavError> {
    if req.method == "OPTIONS" {
        return Ok(options_response());
    }
    match parse_target(req.path, user)? {
        Target::Principal => match req.method {
            "PROPFIND" => principal::principal_propfind(user, req.body),
            _ => Err(DavError::MethodNotAllowed),
        },
        t @ (Target::CalendarHome | Target::Calendar(_) | Target::Event(..)) => {
            calendar_side(req, user, t, calendars).await
        }
        t => book_side(req, user, t, books).await,
    }
}

async fn calendar_side(
    req: &DavRequest<'_>,
    user: &str,
    target: Target,
    store: &dyn CalendarStore,
) -> Result<DavResponse, DavError> {
    let depth = parse_depth(req.depth);
    match (target, req.method) {
        (Target::CalendarHome, "PROPFIND") => {
            caldav::calendar_home_propfind(store, user, depth).await
        }
        (Target::CalendarHome, "REPORT") => {
            caldav::calendar_home_report(store, user, req.body).await
        }
        (Target::Calendar(name), m @ ("PROPFIND" | "REPORT")) => {
            let id = calendar_id(store, user, &name).await?;
            match m {
                "PROPFIND" => caldav::calendar_propfind(store, user, &name, id, depth).await,
                _ => caldav::calendar_report(store, user, &name, id, req.body).await,
            }
        }
        (Target::Event(name, uid), m @ ("GET" | "HEAD" | "PUT" | "DELETE")) => {
            let id = calendar_id(store, user, &name).await?;
            match m {
                "PUT" => {
                    caldav::event_put(store, id, &uid, req.if_match, req.if_none_match, req.body)
                        .await
                }
                "DELETE" => caldav::event_delete(store, id, &uid).await,
                _ => caldav::event_get(store, id, &uid).await,
            }
        }
        _ => Err(DavError::MethodNotAllowed),
    }
}

async fn book_side(
    req: &DavRequest<'_>,
    user: &str,
    target: Target,
    store: &dyn AddressBookStore,
) -> Result<DavResponse, DavError> {
    let depth = parse_depth(req.depth);
    match (target, req.method) {
        (Target::BookHome, "PROPFIND") => {
            carddav::addressbook_home_propfind(store, user, depth).await
        }
        (Target::BookHome, "REPORT") => {
            carddav::addressbook_home_report(store, user, req.body).await
        }
        (Target::Book(name), m @ ("PROPFIND" | "REPORT")) => {
            let id = book_id(store, user, &name).await?;
            match m {
                "PROPFIND" => carddav::addressbook_propfind(store, user, &name, id, depth).await,
                _ => carddav::addressbook_report(store, user, &name, id, req.body).await,
            }
        }
        (Target::Contact(name, uid), m @ ("GET" | "HEAD" | "PUT" | "DELETE")) => {
            let id = book_id(store, user, &name).await?;
            match m {
                "PUT" => {
                    carddav::contact_put(store, id, &uid, req.if_match, req.if_none_match, req.body)
                        .await
                }
                "DELETE" => carddav::contact_delete(store, id, &uid).await,
                _ => carddav::contact_get(store, id, &uid).await,
            }
        }
        _ => Err(DavError::MethodNotAllowed),
    }
}

async fn calendar_id(store: &dyn CalendarStore, user: &str, name: &str) -> Result<i64, DavError> {
    match store
        .get_calendar(user, name)
        .await
        .map_err(into_dav_error)?
    {
        Some(cal) => Ok(cal.id),
        None => Err(DavError::NotFound),
    }
}

async fn book_id(store: &dyn AddressBookStore, user: &str, name: &str) -> Result<i64, DavError> {
    match store
        .get_address_book(user, name)
        .await
        .map_err(into_dav_error)?
    {
        Some(book) => Ok(book.id),
        None => Err(DavError::NotFound),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const U: &str = "alice@example.com";

    #[test]
    fn principal_paths() {
        assert_eq!(parse_target("/dav", U), Ok(Target::Principal));
        assert_eq!(parse_target("/dav/", U), Ok(Target::Principal));
        assert_eq!(parse_target("/dav/principals/", U), Ok(Target::Principal));
        assert_eq!(
            parse_target("/dav/principals/alice%40example.com/", U),
            Ok(Target::Principal)
        );
    }

    #[test]
    fn collection_and_resource_paths_decode() {
        assert_eq!(
            parse_target("/dav/calendars/alice@example.com/My%20Cal/", U),
            Ok(Target::Calendar("My Cal".into()))
        );
        assert_eq!(
            parse_target("/dav/addressbooks/alice@example.com/default/x%2By.vcf", U),
            Ok(Target::Contact("default".into(), "x+y".into()))
        );
    }

    #[test]
    fn another_users_path_is_forbidden() {
        for p in [
            "/dav/principals/bob@example.com/",
            "/dav/calendars/bob@example.com/",
            "/dav/calendars/bob@example.com/default/e.ics",
            "/dav/addressbooks/bob@example.com/default/",
        ] {
            assert!(
                matches!(parse_target(p, U), Err(DavError::Forbidden)),
                "{p}"
            );
        }
    }

    #[test]
    fn unknown_or_wrong_suffix_paths_are_not_found() {
        for p in [
            "/davx/",
            "/dav/files/",
            "/dav/calendars/alice@example.com/default/e.vcf",
            "/dav/calendars/alice@example.com/default/a/b",
        ] {
            assert!(matches!(parse_target(p, U), Err(DavError::NotFound)), "{p}");
        }
    }

    #[test]
    fn a_malformed_escape_is_a_bad_request() {
        assert!(matches!(
            parse_target("/dav/calendars/alice@example.com/%zz/", U),
            Err(DavError::BadRequest(_))
        ));
    }
}
