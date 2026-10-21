//! The address book: one per user, the cards in [`contact_cards`].
//!
//! [`contact_cards`]: crate::families::contact_cards

use async_trait::async_trait;
use mailrs_dav::types::{AddressBook, Contact, PutResult};
use mailrs_dav::{AddressBookStore, StoreError, etag_of};

use super::{DavStore, KevyAccess, bad_request};
use crate::families::contact_cards as cards;

const BOOK_ID: i64 = 0;
const BOOK_NAME: &str = "default";

fn to_contact(uid: String, vcard: String) -> Result<Contact, StoreError> {
    // every stored card passed `parse_card` on its way in
    let fields = cards::parse_card(&vcard).map_err(std::io::Error::other)?;
    Ok(Contact {
        uid,
        etag: etag_of(&vcard),
        fn_name: fields.fn_name,
        email: fields.emails.into_iter().next().unwrap_or_default(),
        vcard,
    })
}

// The book id is not checked: the only book is `BOOK_ID`, and the
// dispatcher only passes ids `get_address_book` returned.
#[async_trait]
impl<A: KevyAccess> AddressBookStore for DavStore<A> {
    async fn list_address_books(&self, user: &str) -> Result<Vec<AddressBook>, StoreError> {
        self.owner(user)?;
        Ok(vec![AddressBook {
            id: BOOK_ID,
            name: BOOK_NAME.into(),
            description: "Contacts".into(),
        }])
    }

    async fn get_address_book(
        &self,
        user: &str,
        name: &str,
    ) -> Result<Option<AddressBook>, StoreError> {
        let all = self.list_address_books(user).await?;
        Ok(all.into_iter().find(|b| b.name == name))
    }

    async fn list_contacts(&self, _book_id: i64) -> Result<Vec<Contact>, StoreError> {
        let all = self.run(cards::list)?;
        all.into_iter().map(|(u, v)| to_contact(u, v)).collect()
    }

    async fn get_contact(&self, _book_id: i64, uid: &str) -> Result<Option<Contact>, StoreError> {
        match self.run(|c, user| cards::get(c, user, uid))? {
            Some(vcard) => Ok(Some(to_contact(uid.to_string(), vcard)?)),
            None => Ok(None),
        }
    }

    async fn contact_etag(&self, book_id: i64, uid: &str) -> Result<Option<String>, StoreError> {
        Ok(self.get_contact(book_id, uid).await?.map(|c| c.etag))
    }

    async fn put_contact(
        &self,
        _book_id: i64,
        uid: &str,
        vcard: &str,
        etag: &str,
    ) -> Result<PutResult, StoreError> {
        cards::parse_card(vcard).map_err(bad_request)?;
        let created = self.run(|c, user| cards::put(c, user, uid, vcard))?;
        Ok(PutResult {
            created,
            etag: etag.to_string(),
        })
    }

    async fn delete_contact(&self, _book_id: i64, uid: &str) -> Result<bool, StoreError> {
        self.run(|c, user| cards::delete(c, user, uid))
    }

    async fn ensure_default_address_book(&self, user: &str) -> Result<(), StoreError> {
        self.owner(user)
    }
}
