//! CardDAV (RFC 6352) request handlers.
//!
//! Same shape as [`crate::caldav`] — `&dyn AddressBookStore` in, [`DavResponse`]
//! out, no axum / sqlx leaking through.

use crate::error::DavError;
use crate::parse::extract_multiget_uids;
use crate::store::AddressBookStore;
use crate::types::PutResult;
use crate::xml::{DavResponse, etag_of, multistatus, xml_escape};

/// PROPFIND on `/dav/contacts/{user}/` — the address-book home collection.
pub async fn addressbook_home_propfind(
    store: &dyn AddressBookStore,
    user: &str,
    depth: u32,
) -> Result<DavResponse, DavError> {
    store
        .ensure_default_address_book(user)
        .await
        .map_err(|e| DavError::ServerError(e.to_string()))?;

    let mut responses = format!(
        "<D:response>\n\
         <D:href>/dav/contacts/{user}/</D:href>\n\
         <D:propstat>\n<D:prop>\n\
         <D:resourcetype><D:collection/></D:resourcetype>\n\
         <D:displayname>Address Books</D:displayname>\n\
         </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
         </D:response>\n"
    );

    if depth >= 1 {
        let books = store
            .list_address_books(user)
            .await
            .map_err(|e| DavError::ServerError(e.to_string()))?;
        for b in &books {
            let encoded_name = urlencode(&b.name);
            let href = format!("/dav/contacts/{user}/{encoded_name}/");
            responses.push_str(&format!(
                "<D:response>\n\
                 <D:href>{href}</D:href>\n\
                 <D:propstat>\n<D:prop>\n\
                 <D:resourcetype><D:collection/><CR:addressbook/></D:resourcetype>\n\
                 <D:displayname>{}</D:displayname>\n\
                 <D:current-user-privilege-set>\
                 <D:privilege><D:all/></D:privilege>\
                 </D:current-user-privilege-set>\n\
                 <CS:getctag>{}</CS:getctag>\n\
                 </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
                 </D:response>\n",
                xml_escape(&b.name),
                xml_escape(&b.description),
            ));
        }
    }

    Ok(multistatus(&responses))
}

/// PROPFIND on `/dav/contacts/{user}/{book}/` — a single address book.
pub async fn addressbook_propfind(
    store: &dyn AddressBookStore,
    user: &str,
    book: &str,
    book_id: i64,
    depth: u32,
) -> Result<DavResponse, DavError> {
    let href = format!("/dav/contacts/{user}/{book}/");
    let mut responses = format!(
        "<D:response>\n\
         <D:href>{href}</D:href>\n\
         <D:propstat>\n<D:prop>\n\
         <D:resourcetype><D:collection/><CR:addressbook/></D:resourcetype>\n\
         <D:displayname>{book}</D:displayname>\n\
         </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
         </D:response>\n"
    );

    if depth >= 1 {
        let contacts = store
            .list_contacts(book_id)
            .await
            .map_err(|e| DavError::ServerError(e.to_string()))?;
        for c in &contacts {
            let contact_href = format!("/dav/contacts/{user}/{book}/{}.vcf", c.uid);
            responses.push_str(&format!(
                "<D:response>\n\
                 <D:href>{}</D:href>\n\
                 <D:propstat>\n<D:prop>\n\
                 <D:getetag>\"{}\"</D:getetag>\n\
                 <D:getcontenttype>text/vcard; charset=utf-8</D:getcontenttype>\n\
                 </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
                 </D:response>\n",
                xml_escape(&contact_href),
                c.etag,
            ));
        }
    }

    Ok(multistatus(&responses))
}

/// REPORT on an address book — both `addressbook-multiget` (RFC 6352 §8.7)
/// and `addressbook-query` (§8.6, no filter support — returns all).
pub async fn addressbook_report(
    store: &dyn AddressBookStore,
    user: &str,
    book: &str,
    book_id: i64,
    body: &str,
) -> Result<DavResponse, DavError> {
    let is_multiget = body.contains("addressbook-multiget");

    let contacts = store
        .list_contacts(book_id)
        .await
        .map_err(|e| DavError::ServerError(e.to_string()))?;

    let filtered: Vec<_> = if is_multiget {
        let uids = extract_multiget_uids(body, ".vcf");
        if uids.is_empty() {
            return Ok(multistatus(""));
        }
        contacts
            .into_iter()
            .filter(|c| uids.iter().any(|u| u == &c.uid))
            .collect()
    } else {
        contacts
    };

    let mut responses = String::new();
    for c in &filtered {
        let contact_href = format!("/dav/contacts/{user}/{book}/{}.vcf", c.uid);
        responses.push_str(&format!(
            "<D:response>\n\
             <D:href>{}</D:href>\n\
             <D:propstat>\n<D:prop>\n\
             <D:getetag>\"{}\"</D:getetag>\n\
             <CR:address-data>{}</CR:address-data>\n\
             </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
             </D:response>\n",
            xml_escape(&contact_href),
            c.etag,
            xml_escape(&c.vcard),
        ));
    }
    Ok(multistatus(&responses))
}

/// GET on `/dav/contacts/{user}/{book}/{uid}.vcf`.
pub async fn contact_get(
    store: &dyn AddressBookStore,
    book_id: i64,
    uid: &str,
) -> Result<DavResponse, DavError> {
    let contact = store
        .get_contact(book_id, uid)
        .await
        .map_err(|e| DavError::ServerError(e.to_string()))?;
    match contact {
        Some(c) => Ok(DavResponse::new(200)
            .with_header("content-type", "text/vcard; charset=utf-8")
            .with_header("etag", &format!("\"{}\"", c.etag))
            .with_body(c.vcard.into_bytes())),
        None => Err(DavError::NotFound),
    }
}

/// PUT on a contact resource. Same precondition handling as event_put.
pub async fn contact_put(
    store: &dyn AddressBookStore,
    book_id: i64,
    uid: &str,
    if_match: Option<&str>,
    if_none_match: Option<&str>,
    body: &str,
) -> Result<DavResponse, DavError> {
    if let Some(expected_raw) = if_match {
        let expected = expected_raw.trim_matches('"');
        let current = store
            .contact_etag(book_id, uid)
            .await
            .map_err(|e| DavError::ServerError(e.to_string()))?;
        match current {
            Some(ref e) if e == expected => {}
            _ => return Err(DavError::PreconditionFailed),
        }
    }

    if let Some(inm) = if_none_match
        && inm.trim() == "*"
    {
        let existing = store
            .contact_etag(book_id, uid)
            .await
            .map_err(|e| DavError::ServerError(e.to_string()))?;
        if existing.is_some() {
            return Err(DavError::PreconditionFailed);
        }
    }

    let etag = etag_of(body);
    let PutResult { created, etag: stored_etag } = store
        .put_contact(book_id, uid, body, &etag)
        .await
        .map_err(|e| DavError::ServerError(e.to_string()))?;

    let status = if created { 201 } else { 204 };
    Ok(DavResponse::new(status).with_header("etag", &format!("\"{stored_etag}\"")))
}

/// DELETE on a contact resource.
pub async fn contact_delete(
    store: &dyn AddressBookStore,
    book_id: i64,
    uid: &str,
) -> Result<DavResponse, DavError> {
    let deleted = store
        .delete_contact(book_id, uid)
        .await
        .map_err(|e| DavError::ServerError(e.to_string()))?;
    if deleted {
        Ok(DavResponse::new(204))
    } else {
        Err(DavError::NotFound)
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AddressBook, Contact};
    use async_trait::async_trait;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemAB {
        books: Mutex<Vec<(String, AddressBook)>>,
        contacts: Mutex<Vec<(i64, Contact)>>,
        next_id: Mutex<i64>,
    }
    impl MemAB {
        fn add_book(&self, owner: &str, name: &str) -> i64 {
            let mut next = self.next_id.lock().unwrap();
            *next += 1;
            let id = *next;
            self.books.lock().unwrap().push((
                owner.into(),
                AddressBook {
                    id,
                    name: name.into(),
                    description: "".into(),
                },
            ));
            id
        }
        fn add_contact(&self, bid: i64, uid: &str, vcard: &str) {
            self.contacts.lock().unwrap().push((
                bid,
                Contact {
                    uid: uid.into(),
                    etag: etag_of(vcard),
                    vcard: vcard.into(),
                    fn_name: "".into(),
                    email: "".into(),
                },
            ));
        }
    }
    #[async_trait]
    impl AddressBookStore for MemAB {
        async fn list_address_books(&self, user: &str) -> Result<Vec<AddressBook>, crate::store::StoreError> {
            Ok(self.books.lock().unwrap().iter().filter(|(o, _)| o == user).map(|(_, b)| b.clone()).collect())
        }
        async fn get_address_book(&self, user: &str, name: &str) -> Result<Option<AddressBook>, crate::store::StoreError> {
            Ok(self.books.lock().unwrap().iter().find(|(o, b)| o == user && b.name == name).map(|(_, b)| b.clone()))
        }
        async fn list_contacts(&self, bid: i64) -> Result<Vec<Contact>, crate::store::StoreError> {
            Ok(self.contacts.lock().unwrap().iter().filter(|(b, _)| *b == bid).map(|(_, c)| c.clone()).collect())
        }
        async fn get_contact(&self, bid: i64, uid: &str) -> Result<Option<Contact>, crate::store::StoreError> {
            Ok(self.contacts.lock().unwrap().iter().find(|(b, c)| *b == bid && c.uid == uid).map(|(_, c)| c.clone()))
        }
        async fn contact_etag(&self, bid: i64, uid: &str) -> Result<Option<String>, crate::store::StoreError> {
            Ok(self.contacts.lock().unwrap().iter().find(|(b, c)| *b == bid && c.uid == uid).map(|(_, c)| c.etag.clone()))
        }
        async fn put_contact(&self, bid: i64, uid: &str, vcard: &str, etag: &str) -> Result<PutResult, crate::store::StoreError> {
            let mut cs = self.contacts.lock().unwrap();
            let pos = cs.iter().position(|(b, c)| *b == bid && c.uid == uid);
            let created = pos.is_none();
            if let Some(p) = pos {
                cs[p].1.vcard = vcard.into();
                cs[p].1.etag = etag.into();
            } else {
                cs.push((bid, Contact {
                    uid: uid.into(),
                    etag: etag.into(),
                    vcard: vcard.into(),
                    fn_name: "".into(),
                    email: "".into(),
                }));
            }
            Ok(PutResult { created, etag: etag.into() })
        }
        async fn delete_contact(&self, bid: i64, uid: &str) -> Result<bool, crate::store::StoreError> {
            let mut cs = self.contacts.lock().unwrap();
            let before = cs.len();
            cs.retain(|(b, c)| !(*b == bid && c.uid == uid));
            Ok(cs.len() < before)
        }
        async fn ensure_default_address_book(&self, user: &str) -> Result<(), crate::store::StoreError> {
            let has = self.books.lock().unwrap().iter().any(|(o, _)| o == user);
            if !has {
                self.add_book(user, "Default");
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn contact_get_returns_vcard() {
        let s = MemAB::default();
        let bid = s.add_book("u", "Friends");
        s.add_contact(bid, "abc", "BEGIN:VCARD\nFN:A\nEND:VCARD");
        let r = contact_get(&s, bid, "abc").await.unwrap();
        assert_eq!(r.status, 200);
        assert!(String::from_utf8(r.body).unwrap().contains("VCARD"));
    }

    #[tokio::test]
    async fn contact_put_then_delete() {
        let s = MemAB::default();
        let bid = s.add_book("u", "Friends");
        let r = contact_put(&s, bid, "abc", None, None, "BEGIN:VCARD\nFN:A\nEND:VCARD")
            .await
            .unwrap();
        assert_eq!(r.status, 201);
        let d = contact_delete(&s, bid, "abc").await.unwrap();
        assert_eq!(d.status, 204);
    }

    #[tokio::test]
    async fn addressbook_report_multiget_filters() {
        let s = MemAB::default();
        let bid = s.add_book("u", "B");
        s.add_contact(bid, "a", "BEGIN:VCARD\nUID:a\nEND:VCARD");
        s.add_contact(bid, "b", "BEGIN:VCARD\nUID:b\nEND:VCARD");
        let body = "<CR:addressbook-multiget xmlns:CR=\"urn:ietf:params:xml:ns:carddav\">\
                    <D:href>/dav/contacts/u/B/a.vcf</D:href></CR:addressbook-multiget>";
        let r = addressbook_report(&s, "u", "B", bid, body).await.unwrap();
        let text = String::from_utf8(r.body).unwrap();
        assert!(text.contains("/B/a.vcf"));
        assert!(!text.contains("/B/b.vcf"));
    }
}
