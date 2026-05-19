//! End-to-end example: create a Maildir in a temp directory, deliver
//! three messages, scan `new/`, then move one to `cur/` with a flag.
//!
//! Run with: `cargo run -p mailrs-maildir --example deliver_and_scan`

use std::fs;

use mailrs_maildir::{Flag, Maildir, serialize_flags};

fn main() -> std::io::Result<()> {
    // pick a temp path; clean it up at the end
    let path = std::env::temp_dir().join("mailrs-maildir-example");
    let _ = fs::remove_dir_all(&path);
    let md = Maildir::create(&path)?;

    // deliver three messages
    for n in 1..=3 {
        let body = format!("From: a{n}@example.com\r\nSubject: msg {n}\r\n\r\nhello\r\n");
        let id = md.deliver(body.as_bytes())?;
        println!("delivered: {id}");
    }

    // scan new/ and print what we got
    let entries = md.scan_new()?;
    println!("\n{} entries in new/:", entries.len());
    for e in &entries {
        println!("  {} flags={:?}", e.id, e.flags);
    }

    // move the first entry to cur/ with the Seen flag set — this is what
    // a client does after the user reads a message
    if let Some(first) = entries.first() {
        let new_name = format!("{}{}", first.id, serialize_flags(&[Flag::Seen]));
        let dest = path.join("cur").join(&new_name);
        fs::rename(&first.path, &dest)?;
        println!("\nmoved {} -> cur/{} with flag Seen", first.id, new_name);
    }

    let cur = md.scan_cur()?;
    println!("\n{} entries in cur/:", cur.len());
    for e in &cur {
        println!("  {} flags={:?}", e.id, e.flags);
    }

    let _ = fs::remove_dir_all(&path);
    Ok(())
}
