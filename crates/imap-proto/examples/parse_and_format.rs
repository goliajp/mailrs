//! End-to-end example: parse a few IMAP commands, expand a sequence set,
//! and format some untagged + tagged responses.
//!
//! Run with: `cargo run -p mailrs-imap-proto --example parse_and_format`
//!
//! No network I/O — the example demonstrates how the parser, sequence-set
//! helpers, and response formatters fit together. A real IMAP server
//! would take parsed commands, look up state (selected mailbox / UIDs),
//! and emit the formatted responses on the wire.

use mailrs_imap_proto::{
    ImapCommand, SequenceSet, format_capability, format_exists, format_fetch, format_flags,
    format_ok, parse_command, parse_sequence_set, sequence_set_to_uids,
};

fn main() {
    // 1. parse a few commands
    let lines = [
        "a001 CAPABILITY",
        "a002 LOGIN alice secret",
        "a003 SELECT INBOX",
        "a004 FETCH 1:3 (FLAGS UID)",
    ];

    for line in lines {
        let parsed = parse_command(line).unwrap();
        println!("{:>4}: {:?}", parsed.tag, parsed.command);
    }

    println!();

    // 2. expand a sequence set against a hypothetical mailbox of 8 messages
    let set = parse_sequence_set("1,3:5,7:*").unwrap();
    let uids = sequence_set_to_uids(&set, 8);
    println!("sequence set '1,3:5,7:*' against 8-message mailbox -> {uids:?}");

    println!();

    // 3. format what we'd write back to the client
    print!("{}", format_capability(&["IMAP4rev1", "IDLE", "AUTH=PLAIN"]));
    print!("{}", format_flags(&["\\Seen", "\\Answered", "\\Flagged"]));
    print!("{}", format_exists(8));
    for &uid in &[1u32, 2, 3] {
        let items = vec![
            ("FLAGS".to_string(), "(\\Seen)".to_string()),
            ("UID".to_string(), uid.to_string()),
        ];
        print!("{}", format_fetch(uid, &items));
    }
    print!("{}", format_ok("a004", "FETCH completed"));

    // 4. demonstrate that SequenceSet is a typed value, not a string
    let _typed_set = SequenceSet::Range(1, 100);

    // and that ImapCommand is exhaustive enough to pattern-match on
    if let ImapCommand::Capability = parse_command("a001 CAPABILITY").unwrap().command {
        println!("\n(Capability matched cleanly via typed enum.)");
    }
}
