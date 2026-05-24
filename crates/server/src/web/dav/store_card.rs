//! `AddressBookStore` impl for `DavAdapter`.

use async_trait::async_trait;

use mailrs_dav::store::{AddressBookStore, StoreError};
use mailrs_dav::types::{AddressBook, Contact, PutResult};

use super::{DavAdapter, to_store_err};


#[async_trait]
impl AddressBookStore for DavAdapter {
    async fn list_address_books(&self, user: &str) -> Result<Vec<AddressBook>, StoreError> {
        let rows = sqlx::query_as::<_, (i64, String, String)>(
            "SELECT id, name, description FROM address_books \
             WHERE account_address = $1 ORDER BY name",
        )
        .bind(user)
        .fetch_all(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(rows
            .into_iter()
            .map(|(id, name, description)| AddressBook {
                id,
                name,
                description,
            })
            .collect())
    }

    async fn get_address_book(
        &self,
        user: &str,
        book_name: &str,
    ) -> Result<Option<AddressBook>, StoreError> {
        let row = sqlx::query_as::<_, (i64, String, String)>(
            "SELECT id, name, description FROM address_books \
             WHERE account_address = $1 AND name = $2",
        )
        .bind(user)
        .bind(book_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(row.map(|(id, name, description)| AddressBook {
            id,
            name,
            description,
        }))
    }

    async fn list_contacts(&self, book_id: i64) -> Result<Vec<Contact>, StoreError> {
        let rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT uid, etag, vcard FROM contacts WHERE address_book_id = $1",
        )
        .bind(book_id)
        .fetch_all(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(rows
            .into_iter()
            .map(|(uid, etag, vcard)| Contact {
                uid,
                etag,
                vcard,
                fn_name: String::new(),
                email: String::new(),
            })
            .collect())
    }

    async fn get_contact(
        &self,
        book_id: i64,
        uid: &str,
    ) -> Result<Option<Contact>, StoreError> {
        let row = sqlx::query_as::<_, (String, String)>(
            "SELECT etag, vcard FROM contacts WHERE address_book_id = $1 AND uid = $2",
        )
        .bind(book_id)
        .bind(uid)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(row.map(|(etag, vcard)| Contact {
            uid: uid.to_string(),
            etag,
            vcard,
            fn_name: String::new(),
            email: String::new(),
        }))
    }

    async fn contact_etag(
        &self,
        book_id: i64,
        uid: &str,
    ) -> Result<Option<String>, StoreError> {
        let etag: Option<String> = sqlx::query_scalar(
            "SELECT etag FROM contacts WHERE address_book_id = $1 AND uid = $2",
        )
        .bind(book_id)
        .bind(uid)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(etag)
    }

    async fn put_contact(
        &self,
        book_id: i64,
        uid: &str,
        vcard: &str,
        etag: &str,
    ) -> Result<PutResult, StoreError> {
        let existed: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM contacts WHERE address_book_id = $1 AND uid = $2)",
        )
        .bind(book_id)
        .bind(uid)
        .fetch_one(&self.pool)
        .await
        .map_err(to_store_err)?;

        let fn_name = mailrs_dav::parse::extract_vcard_field(vcard, "FN");
        let email = mailrs_dav::parse::extract_vcard_field(vcard, "EMAIL");

        sqlx::query(
            "INSERT INTO contacts (address_book_id, uid, etag, vcard, fn_name, email)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (address_book_id, uid)
             DO UPDATE SET etag = $3, vcard = $4, fn_name = $5, email = $6, updated_at = now()",
        )
        .bind(book_id)
        .bind(uid)
        .bind(etag)
        .bind(vcard)
        .bind(&fn_name)
        .bind(&email)
        .execute(&self.pool)
        .await
        .map_err(to_store_err)?;

        Ok(PutResult {
            created: !existed,
            etag: etag.to_string(),
        })
    }

    async fn delete_contact(&self, book_id: i64, uid: &str) -> Result<bool, StoreError> {
        let res = sqlx::query("DELETE FROM contacts WHERE address_book_id = $1 AND uid = $2")
            .bind(book_id)
            .bind(uid)
            .execute(&self.pool)
            .await
            .map_err(to_store_err)?;
        Ok(res.rows_affected() > 0)
    }

    async fn ensure_default_address_book(&self, user: &str) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO address_books (account_address, name) VALUES ($1, 'Default') ON CONFLICT DO NOTHING",
        )
        .bind(user)
        .execute(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(())
    }
}
