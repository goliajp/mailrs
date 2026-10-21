//! Address-book cards, as CardDAV stores them, and the addresses they add
//! to recipient autocomplete.
//!
//! ```text
//!   contact_cards:{user}          hash  uid → vCard text
//!   mailrs:user:{user}:contacts   hash  email → "Name <email>"  (autocomplete)
//! ```
//!
//! The autocomplete hash is what the web's recipient field reads, and what
//! the greylist checks to recognise a known correspondent. Mail arrival
//! fills it from senders; a card adds each of its addresses, so a contact
//! saved on a phone is offered when writing mail. [`put`] writes both.
//!
//! Deleting a card leaves its autocomplete entries: an address in that hash
//! may equally have come from a message, and the hash does not record which.

use std::io;

use kevy_client::Connection;

/// The autocomplete hash `GET /api/contacts` reads.
pub fn autocomplete_key(user: &str) -> String {
    format!("mailrs:user:{user}:contacts")
}

/// The hash holding `user`'s cards.
pub fn cards_key(user: &str) -> String {
    format!("contact_cards:{user}")
}

/// What a card contributes beyond its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardFields {
    /// `FN`, the formatted name.
    pub fn_name: String,
    /// Every `EMAIL`, lowercased, in order.
    pub emails: Vec<String>,
}

/// Read `FN` and the `EMAIL`s out of a vCard.
///
/// `Err` when the text is not a vCard at all. Folded lines are unfolded
/// first (RFC 6350 §3.2), and grouped properties (`item1.EMAIL`, which
/// Apple writes) count.
pub fn parse_card(vcard: &str) -> Result<CardFields, String> {
    let unfolded = vcard
        .replace("\r\n ", "")
        .replace("\r\n\t", "")
        .replace("\n ", "")
        .replace("\n\t", "");
    let lines: Vec<&str> = unfolded.lines().map(str::trim_end).collect();
    let begins = lines.iter().any(|l| l.eq_ignore_ascii_case("BEGIN:VCARD"));
    let ends = lines.iter().any(|l| l.eq_ignore_ascii_case("END:VCARD"));
    if !begins || !ends {
        return Err("not a vCard: BEGIN:VCARD / END:VCARD missing".into());
    }
    let mut fields = CardFields {
        fn_name: String::new(),
        emails: Vec::new(),
    };
    for line in lines {
        let Some((head, value)) = line.split_once(':') else {
            continue;
        };
        let name = head.split(';').next().unwrap_or(head);
        let name = name.rsplit('.').next().unwrap_or(name);
        if name.eq_ignore_ascii_case("FN") {
            fields.fn_name = value.trim().to_string();
        } else if name.eq_ignore_ascii_case("EMAIL") && value.contains('@') {
            fields.emails.push(value.trim().to_lowercase());
        }
    }
    Ok(fields)
}

/// The autocomplete value for one address of a card.
fn display(fields: &CardFields, email: &str) -> String {
    match fields.fn_name.is_empty() {
        true => email.to_string(),
        false => format!("{} <{email}>", fields.fn_name),
    }
}

/// Store a card and offer its addresses to autocomplete. `Ok(true)` when
/// the card is new.
pub fn put(conn: &mut Connection, user: &str, uid: &str, vcard: &str) -> io::Result<bool> {
    let fields = parse_card(vcard).map_err(io::Error::other)?;
    let created = conn.hset(
        cards_key(user).as_bytes(),
        &[(uid.as_bytes(), vcard.as_bytes())],
    )? > 0;
    if !fields.emails.is_empty() {
        let pairs: Vec<(String, String)> = fields
            .emails
            .iter()
            .map(|e| (e.clone(), display(&fields, e)))
            .collect();
        let refs: Vec<(&[u8], &[u8])> = pairs
            .iter()
            .map(|(k, v)| (k.as_bytes(), v.as_bytes()))
            .collect();
        conn.hset(autocomplete_key(user).as_bytes(), &refs)?;
    }
    Ok(created)
}

/// One card's text.
pub fn get(conn: &mut Connection, user: &str, uid: &str) -> io::Result<Option<String>> {
    let raw = conn.hget(cards_key(user).as_bytes(), uid.as_bytes())?;
    Ok(raw.map(|v| String::from_utf8_lossy(&v).into_owned()))
}

/// Every card of `user`'s, as `(uid, text)`.
pub fn list(conn: &mut Connection, user: &str) -> io::Result<Vec<(String, String)>> {
    let flat = conn.hgetall(cards_key(user).as_bytes())?;
    Ok(flat
        .chunks(2)
        .filter_map(|pair| match pair {
            [k, v] => Some((
                String::from_utf8_lossy(k).into_owned(),
                String::from_utf8_lossy(v).into_owned(),
            )),
            _ => None,
        })
        .collect())
}

/// Remove a card. `true` when it existed.
pub fn delete(conn: &mut Connection, user: &str, uid: &str) -> io::Result<bool> {
    Ok(conn.hdel(cards_key(user).as_bytes(), &[uid.as_bytes()])? > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARD: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Ada Love\r\n lace\r\n\
        item1.EMAIL;TYPE=INTERNET:Ada@Example.com\r\nEMAIL:ada@work.example\r\n\
        TEL:+81 3 0000\r\nEND:VCARD\r\n";

    #[test]
    fn name_and_every_address_are_read() {
        let f = parse_card(CARD).expect("card");
        assert_eq!(f.fn_name, "Ada Lovelace");
        assert_eq!(f.emails, ["ada@example.com", "ada@work.example"]);
    }

    #[test]
    fn text_that_is_not_a_card_is_refused() {
        assert!(parse_card("BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n").is_err());
        assert!(parse_card("").is_err());
    }

    #[test]
    fn put_fills_autocomplete_and_reports_creation() {
        let mut c = Connection::connect("mem://contact-cards-put").expect("kevy");
        assert!(put(&mut c, "u", "c1", CARD).expect("put"));
        assert!(!put(&mut c, "u", "c1", CARD).expect("put again"));
        assert_eq!(get(&mut c, "u", "c1").expect("get").as_deref(), Some(CARD));
        let auto = c
            .hget(autocomplete_key("u").as_bytes(), b"ada@example.com")
            .expect("hget");
        assert_eq!(
            auto.as_deref(),
            Some(&b"Ada Lovelace <ada@example.com>"[..])
        );
        assert!(delete(&mut c, "u", "c1").expect("delete"));
        assert!(list(&mut c, "u").expect("list").is_empty());
    }
}
