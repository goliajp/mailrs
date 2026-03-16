#!/usr/bin/env python3
"""Import .eml sample files into local mailrs (Maildir + PostgreSQL)."""

import email
import email.policy
import email.utils
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

SAMPLE_DIR = Path(__file__).resolve().parent.parent / "samples"
MAILDIR_ROOT = Path("/tmp/mailrs/maildir")
USER = "lihao@golia.jp"
LOCAL, DOMAIN = USER.split("@")
MAILDIR_PATH = MAILDIR_ROOT / DOMAIN / LOCAL
PG_CONTAINER = "mailrs-postgres"
USERS_FILE = Path("/tmp/mailrs/users.toml")

# sequence counter for generating unique maildir filenames
_seq = 0


def generate_maildir_id():
    global _seq
    ts = int(time.time())
    _seq += 1
    return f"{ts}.M000000P{os.getpid()}Q{_seq}.import"


def psql(sql):
    result = subprocess.run(
        ["docker", "exec", "-i", PG_CONTAINER, "psql", "-U", "mailrs", "-d", "mailrs", "-t", "-A"],
        input=sql,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        print(f"  psql error: {result.stderr.strip()}", file=sys.stderr)
    return result.stdout.strip()


def extract_header_raw(data: bytes, name: str) -> str:
    """extract a raw header value from RFC 5322 bytes (matches mailrs behavior)."""
    target = name.lower().encode() + b":"
    lines = data.split(b"\n")
    found = False
    value_parts = []
    for line in lines:
        stripped = line.rstrip(b"\r")
        if stripped == b"":
            break
        if found:
            if stripped[:1] in (b" ", b"\t"):
                value_parts.append(stripped.decode("utf-8", errors="replace").strip())
            else:
                break
        if stripped.lower().startswith(target):
            val = stripped[len(target):].decode("utf-8", errors="replace").strip()
            value_parts.append(val)
            found = True
    return " ".join(value_parts)


def normalize_message_id(raw: str) -> str:
    """strip angle brackets: <abc@host> -> abc@host"""
    raw = raw.strip()
    if raw.startswith("<") and raw.endswith(">"):
        return raw[1:-1]
    return raw


def parse_date_epoch(data: bytes) -> int:
    """parse Date header to unix timestamp."""
    date_str = extract_header_raw(data, "Date")
    if date_str:
        try:
            parsed = email.utils.parsedate_to_datetime(date_str)
            return int(parsed.timestamp())
        except Exception:
            pass
    return int(time.time())


def sql_escape(s: str) -> str:
    """escape single quotes for SQL."""
    return s.replace("'", "''")


def classify_folder(eml_filename: str) -> str:
    """determine target mailbox from filename prefix."""
    if eml_filename.startswith("sent_"):
        return "Sent"
    if eml_filename.startswith("trash_"):
        return "Trash"
    if eml_filename.startswith("archive_"):
        return "Archive"
    return "INBOX"


def main():
    # ensure maildir directories exist
    for sub in ["tmp", "new", "cur"]:
        (MAILDIR_PATH / sub).mkdir(parents=True, exist_ok=True)
    print(f"Maildir: {MAILDIR_PATH}")

    # setup users.toml
    USERS_FILE.parent.mkdir(parents=True, exist_ok=True)
    USERS_FILE.write_text(
        '[users."lihao@golia.jp"]\npassword = "test123"\n'
    )
    print(f"Users file: {USERS_FILE}")

    # setup domain and account in PG (password_hash stores plaintext for dev)
    psql(f"""
        INSERT INTO domains (name) VALUES ('{DOMAIN}') ON CONFLICT DO NOTHING;
        INSERT INTO accounts (address, domain, display_name, password_hash)
            VALUES ('{USER}', '{DOMAIN}', 'Li Hao', 'test123')
            ON CONFLICT (address) DO UPDATE SET password_hash = 'test123';
    """)

    # create default mailboxes
    now_ts = int(time.time())
    for folder in ["INBOX", "Sent", "Drafts", "Trash", "Junk", "Archive"]:
        psql(f"""
            INSERT INTO mailboxes (user_address, name, uidvalidity, uidnext, highest_modseq)
            VALUES ('{USER}', '{folder}', {now_ts}, 1, 0)
            ON CONFLICT (user_address, name) DO NOTHING;
        """)
    print("Mailboxes created")

    # clear existing messages (fresh import)
    psql(f"""
        DELETE FROM messages WHERE mailbox_id IN (
            SELECT id FROM mailboxes WHERE user_address = '{USER}'
        );
        UPDATE mailboxes SET uidnext = 1, highest_modseq = 0
            WHERE user_address = '{USER}';
    """)

    # clean existing maildir files
    for sub in ["new", "cur"]:
        d = MAILDIR_PATH / sub
        for f in d.iterdir():
            if f.is_file():
                f.unlink()
    print("Cleared existing data")

    # collect eml files sorted by date
    eml_files = sorted(SAMPLE_DIR.glob("*.eml"))
    print(f"\nImporting {len(eml_files)} emails...")

    # build thread lookup: message_id -> thread_id
    thread_map = {}

    # first pass: parse all emails and sort by date
    emails = []
    for eml_path in eml_files:
        data = eml_path.read_bytes()
        folder = classify_folder(eml_path.name)
        date_epoch = parse_date_epoch(data)
        emails.append((eml_path, data, folder, date_epoch))

    # sort by date for correct threading
    emails.sort(key=lambda x: x[3])

    imported = 0
    errors = 0
    folder_counts = {}

    for eml_path, data, folder, date_epoch in emails:
        try:
            sender = extract_header_raw(data, "From")
            recipients = extract_header_raw(data, "To")
            subject = extract_header_raw(data, "Subject")
            raw_msg_id = extract_header_raw(data, "Message-ID")
            raw_in_reply_to = extract_header_raw(data, "In-Reply-To")

            message_id = normalize_message_id(raw_msg_id)
            in_reply_to = normalize_message_id(raw_in_reply_to)
            size = len(data)

            # resolve thread_id (same logic as mailrs)
            if message_id:
                if in_reply_to and in_reply_to in thread_map:
                    thread_id = thread_map[in_reply_to]
                elif in_reply_to:
                    thread_id = in_reply_to
                else:
                    thread_id = message_id
                thread_map[message_id] = thread_id
            else:
                thread_id = ""

            # write to maildir cur/ (mark as Seen)
            maildir_id = generate_maildir_id()
            dest = MAILDIR_PATH / "cur" / f"{maildir_id}:2,S"
            dest.write_bytes(data)

            # insert into PG via batch SQL
            sql = f"""
                WITH mb AS (
                    SELECT id, uidnext, highest_modseq
                    FROM mailboxes
                    WHERE user_address = '{USER}' AND name = '{folder}'
                    FOR UPDATE
                )
                INSERT INTO messages (mailbox_id, uid, maildir_id, sender, recipients, subject,
                                      size, date_epoch, internal_date, message_id, in_reply_to,
                                      thread_id, modseq, flags)
                SELECT mb.id, mb.uidnext, '{sql_escape(maildir_id)}',
                       '{sql_escape(sender)}', '{sql_escape(recipients)}',
                       '{sql_escape(subject)}', {size}, {date_epoch}, {date_epoch},
                       '{sql_escape(message_id)}', '{sql_escape(in_reply_to)}',
                       '{sql_escape(thread_id)}', mb.highest_modseq + 1, 1
                FROM mb;

                UPDATE mailboxes SET uidnext = uidnext + 1, highest_modseq = highest_modseq + 1
                WHERE user_address = '{USER}' AND name = '{folder}';
            """
            psql(sql)

            imported += 1
            folder_counts[folder] = folder_counts.get(folder, 0) + 1

            if imported % 50 == 0:
                print(f"  {imported}/{len(emails)}...", flush=True)

        except Exception as e:
            errors += 1
            print(f"  error {eml_path.name}: {e}", file=sys.stderr)

    print(f"\nDone! Imported {imported}, errors {errors}")
    for folder, count in sorted(folder_counts.items()):
        print(f"  {folder}: {count}")

    # verify
    result = psql(f"""
        SELECT m.name, count(*) FROM messages msg
        JOIN mailboxes m ON msg.mailbox_id = m.id
        WHERE m.user_address = '{USER}'
        GROUP BY m.name ORDER BY m.name;
    """)
    print(f"\nDB verification:\n{result}")

    maildir_count = sum(1 for _ in (MAILDIR_PATH / "cur").iterdir())
    print(f"Maildir files: {maildir_count}")


if __name__ == "__main__":
    main()
