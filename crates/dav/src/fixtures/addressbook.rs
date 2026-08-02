//! The in-memory `AddressBookStore` used by the DAV tests.

use std::sync::RwLock;

use async_trait::async_trait;

use crate::store::{AddressBookStore, StoreError};
use crate::types::{AddressBook, Contact, PutResult};

// =====================================================================
// AddressBook store
// =====================================================================

/// In-memory [`AddressBookStore`] backed by `Vec`s under an `RwLock`. Build
/// via [`Self::new`] + chainable `with_*` setters.
pub struct InMemoryAddressBookStore {
    inner: RwLock<AbInner>,
}

struct AbInner {
    books: Vec<(String, AddressBook)>,
    contacts: Vec<(i64, Contact)>,
    default_created_for: Vec<String>,

    list_books_error: Option<String>,
    get_book_error: Option<String>,
    list_contacts_error: Option<String>,
    get_contact_error: Option<String>,
    contact_etag_error: Option<String>,
    put_contact_error: Option<String>,
    delete_contact_error: Option<String>,
    ensure_default_error: Option<String>,
}

impl InMemoryAddressBookStore {
    /// Construct an empty store.
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(AbInner {
                books: Vec::new(),
                contacts: Vec::new(),
                default_created_for: Vec::new(),
                list_books_error: None,
                get_book_error: None,
                list_contacts_error: None,
                get_contact_error: None,
                contact_etag_error: None,
                put_contact_error: None,
                delete_contact_error: None,
                ensure_default_error: None,
            }),
        }
    }

    /// Append an address book owned by `owner`. Use [`make_book`] for a sane default shape.
    pub fn with_book(self, owner: &str, book: AddressBook) -> Self {
        self.inner
            .write()
            .unwrap()
            .books
            .push((owner.to_string(), book));
        self
    }

    /// Append a contact to the given book id.
    pub fn with_contact(self, book_id: i64, contact: Contact) -> Self {
        self.inner
            .write()
            .unwrap()
            .contacts
            .push((book_id, contact));
        self
    }

    /// Make [`AddressBookStore::list_address_books`] return an error carrying `msg`.
    pub fn list_books_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().list_books_error = Some(msg.to_string());
        self
    }

    /// Make [`AddressBookStore::get_address_book`] return an error carrying `msg`.
    pub fn get_book_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().get_book_error = Some(msg.to_string());
        self
    }

    /// Make [`AddressBookStore::list_contacts`] return an error carrying `msg`.
    pub fn list_contacts_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().list_contacts_error = Some(msg.to_string());
        self
    }

    /// Make [`AddressBookStore::get_contact`] return an error carrying `msg`.
    pub fn get_contact_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().get_contact_error = Some(msg.to_string());
        self
    }

    /// Make [`AddressBookStore::contact_etag`] return an error carrying `msg`.
    pub fn contact_etag_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().contact_etag_error = Some(msg.to_string());
        self
    }

    /// Make [`AddressBookStore::put_contact`] return an error carrying `msg`.
    pub fn put_contact_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().put_contact_error = Some(msg.to_string());
        self
    }

    /// Make [`AddressBookStore::delete_contact`] return an error carrying `msg`.
    pub fn delete_contact_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().delete_contact_error = Some(msg.to_string());
        self
    }

    /// Make the `ensure_default_*` trait method return an error carrying `msg`.
    pub fn ensure_default_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().ensure_default_error = Some(msg.to_string());
        self
    }

    /// Read back every contact currently stored for `book_id`.
    pub fn contacts_in(&self, book_id: i64) -> Vec<Contact> {
        self.inner
            .read()
            .unwrap()
            .contacts
            .iter()
            .filter(|(b, _)| *b == book_id)
            .map(|(_, c)| c.clone())
            .collect()
    }
}

impl Default for InMemoryAddressBookStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AddressBookStore for InMemoryAddressBookStore {
    async fn list_address_books(&self, user: &str) -> Result<Vec<AddressBook>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.list_books_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .books
            .iter()
            .filter(|(o, _)| o == user)
            .map(|(_, b)| b.clone())
            .collect())
    }

    async fn get_address_book(
        &self,
        user: &str,
        book_name: &str,
    ) -> Result<Option<AddressBook>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.get_book_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .books
            .iter()
            .find(|(o, b)| o == user && b.name == book_name)
            .map(|(_, b)| b.clone()))
    }

    async fn list_contacts(&self, book_id: i64) -> Result<Vec<Contact>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.list_contacts_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .contacts
            .iter()
            .filter(|(b, _)| *b == book_id)
            .map(|(_, c)| c.clone())
            .collect())
    }

    async fn get_contact(&self, book_id: i64, uid: &str) -> Result<Option<Contact>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.get_contact_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .contacts
            .iter()
            .find(|(b, c)| *b == book_id && c.uid == uid)
            .map(|(_, c)| c.clone()))
    }

    async fn contact_etag(&self, book_id: i64, uid: &str) -> Result<Option<String>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.contact_etag_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .contacts
            .iter()
            .find(|(b, c)| *b == book_id && c.uid == uid)
            .map(|(_, c)| c.etag.clone()))
    }

    async fn put_contact(
        &self,
        book_id: i64,
        uid: &str,
        vcard: &str,
        etag: &str,
    ) -> Result<PutResult, StoreError> {
        let mut inner = self.inner.write().unwrap();
        if let Some(ref msg) = inner.put_contact_error {
            return Err(msg.clone().into());
        }
        let pos = inner
            .contacts
            .iter()
            .position(|(b, c)| *b == book_id && c.uid == uid);
        let created = pos.is_none();
        if let Some(p) = pos {
            inner.contacts[p].1.vcard = vcard.to_string();
            inner.contacts[p].1.etag = etag.to_string();
        } else {
            inner.contacts.push((
                book_id,
                Contact {
                    uid: uid.to_string(),
                    etag: etag.to_string(),
                    vcard: vcard.to_string(),
                    fn_name: String::new(),
                    email: String::new(),
                },
            ));
        }
        Ok(PutResult {
            created,
            etag: etag.to_string(),
        })
    }

    async fn delete_contact(&self, book_id: i64, uid: &str) -> Result<bool, StoreError> {
        let mut inner = self.inner.write().unwrap();
        if let Some(ref msg) = inner.delete_contact_error {
            return Err(msg.clone().into());
        }
        let before = inner.contacts.len();
        inner
            .contacts
            .retain(|(b, c)| !(*b == book_id && c.uid == uid));
        Ok(inner.contacts.len() < before)
    }

    async fn ensure_default_address_book(&self, user: &str) -> Result<(), StoreError> {
        let mut inner = self.inner.write().unwrap();
        if let Some(ref msg) = inner.ensure_default_error {
            return Err(msg.clone().into());
        }
        let has = inner.books.iter().any(|(o, _)| o == user);
        if !has {
            let next_id = (inner.books.len() as i64) + 1;
            inner.books.push((
                user.to_string(),
                AddressBook {
                    id: next_id,
                    name: "Default".to_string(),
                    description: String::new(),
                },
            ));
            inner.default_created_for.push(user.to_string());
        }
        Ok(())
    }
}
