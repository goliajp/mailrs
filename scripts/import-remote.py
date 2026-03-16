#!/usr/bin/env python3
"""Import .eml files from downloaded email archive into remote mailrs server.

Usage: python3 scripts/import-remote.py /path/to/emails [--dry-run]

The emails directory should have structure:
  user@domain/INBOX/*.eml
  user@domain/Sent/*.eml
  ...
"""

import email.utils
import os
import subprocess
import sys
import time
from pathlib import Path

SSH_KEY = os.environ.get("SSH_KEY", os.path.expanduser("~/keys/aws.pem"))
SSH_HOST = os.environ.get("SSH_HOST", "root@t02.golia.jp")
REMOTE_DIR = "/apps/mailrs"
REMOTE_MAILDIR = "/data/maildir"
PG_CONTAINER = "mailrs-postgres"

SSH_OPTS = ["-i", SSH_KEY, "-o", "StrictHostKeyChecking=no"]

# sequence counter
_seq = 0


def generate_maildir_id():
    global _seq
    ts = int(time.time())
    _seq += 1
    return f"{ts}.M000000P{os.getpid()}Q{_seq}.import"


def ssh_cmd(cmd):
    result = subprocess.run(
        ["ssh"] + SSH_OPTS + [SSH_HOST, cmd],
        capture_output=True, text=True
    )
    if result.returncode != 0 and result.stderr.strip():
        print(f"  ssh error: {result.stderr.strip()}", file=sys.stderr)
    return result.stdout.strip()


def remote_psql(sql):
    cmd = ["ssh"] + SSH_OPTS + [
        SSH_HOST,
        f"cd {REMOTE_DIR} && docker compose exec -T postgres psql -U mailrs -d mailrs -t -A"
    ]
    result = subprocess.run(cmd, input=sql, capture_output=True, text=True)
    if result.returncode != 0 and result.stderr.strip():
        # filter out NOTICEs
        errs = [l for l in result.stderr.strip().split('\n') if 'NOTICE' not in l]
        if errs:
            print(f"  psql error: {'; '.join(errs)}", file=sys.stderr)
    return result.stdout.strip()


def scp_upload(local_path, remote_path):
    result = subprocess.run(
        ["scp"] + SSH_OPTS + [str(local_path), f"{SSH_HOST}:{remote_path}"],
        capture_output=True, text=True
    )
    return result.returncode == 0


def extract_header_raw(data: bytes, name: str) -> str:
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
    raw = raw.strip()
    if raw.startswith("<") and raw.endswith(">"):
        return raw[1:-1]
    return raw


def parse_date_epoch(data: bytes) -> int:
    date_str = extract_header_raw(data, "Date")
    if date_str:
        try:
            parsed = email.utils.parsedate_to_datetime(date_str)
            return int(parsed.timestamp())
        except Exception:
            pass
    return int(time.time())


def sql_escape(s: str) -> str:
    return s.replace("'", "''")


def import_account(account_dir: Path, dry_run: bool = False):
    user = account_dir.name
    local, domain = user.split("@")

    # collect all .eml files across folders
    emails = []
    for folder_dir in sorted(account_dir.iterdir()):
        if not folder_dir.is_dir():
            continue
        folder = folder_dir.name
        for eml_path in sorted(folder_dir.glob("*.eml")):
            data = eml_path.read_bytes()
            date_epoch = parse_date_epoch(data)
            emails.append((eml_path, data, folder, date_epoch))

    if not emails:
        print(f"  {user}: no emails, skipping")
        return 0

    # sort by date for correct threading
    emails.sort(key=lambda x: x[3])
    print(f"  {user}: {len(emails)} emails to import")

    if dry_run:
        return len(emails)

    # create mailboxes
    now_ts = int(time.time())
    folders = sorted(set(f for _, _, f, _ in emails))
    # always ensure standard folders exist
    for f in ["INBOX", "Sent", "Drafts", "Trash", "Junk", "Archive"]:
        if f not in folders:
            folders.append(f)

    for folder in folders:
        remote_psql(f"""
            INSERT INTO mailboxes (user_address, name, uidvalidity, uidnext, highest_modseq)
            VALUES ('{sql_escape(user)}', '{sql_escape(folder)}', {now_ts}, 1, 0)
            ON CONFLICT (user_address, name) DO NOTHING;
        """)

    # create remote maildir
    remote_maildir = f"{REMOTE_MAILDIR}/{domain}/{local}"
    ssh_cmd(f"mkdir -p {remote_maildir}/{{tmp,new,cur}}")

    # batch: pack eml files into a tar, upload, extract
    import tempfile
    import tarfile

    thread_map = {}
    sql_batch = []
    tar_entries = []  # (maildir_id, folder, data, metadata)

    for eml_path, data, folder, date_epoch in emails:
        sender = extract_header_raw(data, "From")
        recipients = extract_header_raw(data, "To")
        subject = extract_header_raw(data, "Subject")
        raw_msg_id = extract_header_raw(data, "Message-ID")
        raw_in_reply_to = extract_header_raw(data, "In-Reply-To")

        message_id = normalize_message_id(raw_msg_id)
        in_reply_to = normalize_message_id(raw_in_reply_to)
        size = len(data)

        # threading
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

        maildir_id = generate_maildir_id()
        tar_entries.append((maildir_id, data))

        sql_batch.append(f"""
            WITH mb AS (
                SELECT id, uidnext, highest_modseq FROM mailboxes
                WHERE user_address = '{sql_escape(user)}' AND name = '{sql_escape(folder)}'
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
            WHERE user_address = '{sql_escape(user)}' AND name = '{sql_escape(folder)}';
        """)

    # create tar with all eml files (as maildir cur/ entries)
    with tempfile.NamedTemporaryFile(suffix=".tar.gz", delete=False) as tmp:
        tar_path = tmp.name
        with tarfile.open(tar_path, "w:gz") as tar:
            for maildir_id, data in tar_entries:
                import io
                filename = f"cur/{maildir_id}:2,S"
                info = tarfile.TarInfo(name=filename)
                info.size = len(data)
                tar.addfile(info, io.BytesIO(data))
    tar_size_mb = os.path.getsize(tar_path) / (1024 * 1024)
    print(f"    tar: {tar_size_mb:.1f} MB")

    # upload and extract tar
    remote_tar = f"/tmp/mailrs-import-{local}.tar.gz"
    print(f"    uploading tar...")
    if not scp_upload(tar_path, remote_tar):
        print(f"    FAILED to upload tar", file=sys.stderr)
        os.unlink(tar_path)
        return 0
    os.unlink(tar_path)

    # extract into maildir on host (docker volume path)
    host_maildir = f"/var/lib/docker/volumes/mailrs_mailrs-data/_data/maildir/{domain}/{local}"
    print(f"    extracting on server...")
    ssh_cmd(f"mkdir -p {host_maildir}/{{tmp,new,cur}} && tar xzf {remote_tar} -C {host_maildir}")
    ssh_cmd(f"rm -f {remote_tar}")

    # insert into PG in batches
    batch_size = 200
    total = len(sql_batch)
    print(f"    inserting {total} records into PG...")
    for i in range(0, total, batch_size):
        chunk = sql_batch[i:i + batch_size]
        combined = "BEGIN;\n" + "\n".join(chunk) + "\nCOMMIT;"
        remote_psql(combined)
        done = min(i + batch_size, total)
        if done % 1000 == 0 or done == total:
            print(f"      {done}/{total}")

    print(f"    done: {total} emails imported")
    return total


def main():
    if len(sys.argv) < 2:
        print("Usage: python3 import-remote.py /path/to/emails [--dry-run]")
        sys.exit(1)

    emails_dir = Path(sys.argv[1])
    dry_run = "--dry-run" in sys.argv

    if not emails_dir.is_dir():
        print(f"Error: {emails_dir} is not a directory")
        sys.exit(1)

    # find account directories
    account_dirs = sorted([
        d for d in emails_dir.iterdir()
        if d.is_dir() and "@" in d.name
    ])

    print(f"Found {len(account_dirs)} accounts")
    if dry_run:
        print("DRY RUN - no changes will be made\n")

    total = 0
    for account_dir in account_dirs:
        count = import_account(account_dir, dry_run)
        total += count

    print(f"\nTotal: {total} emails {'would be ' if dry_run else ''}imported")


if __name__ == "__main__":
    main()
