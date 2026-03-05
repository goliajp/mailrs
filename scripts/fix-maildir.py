#!/usr/bin/env python3
"""Re-upload maildir files that failed to extract (tar was inside container, not host).
Only uploads .eml files as maildir entries — PG data already exists."""

import email.utils
import io
import os
import subprocess
import sys
import tarfile
import tempfile
import time
from pathlib import Path

SSH_KEY = os.environ.get("SSH_KEY", os.path.expanduser("~/keys/aws.pem"))
SSH_HOST = os.environ.get("SSH_HOST", "root@t02.golia.jp")
SSH_OPTS = ["-i", SSH_KEY, "-o", "StrictHostKeyChecking=no"]
HOST_VOLUME = "/var/lib/docker/volumes/mailrs_mailrs-data/_data/maildir"

_seq = 0


def ssh_cmd(cmd):
    result = subprocess.run(
        ["ssh"] + SSH_OPTS + [SSH_HOST, cmd],
        capture_output=True, text=True
    )
    return result.stdout.strip()


def scp_upload(local_path, remote_path):
    result = subprocess.run(
        ["scp"] + SSH_OPTS + [str(local_path), f"{SSH_HOST}:{remote_path}"],
        capture_output=True, text=True
    )
    return result.returncode == 0


def remote_psql(sql):
    cmd = ["ssh"] + SSH_OPTS + [
        SSH_HOST,
        "cd /apps/mailrs && docker compose exec -T postgres psql -U mailrs -d mailrs -t -A"
    ]
    result = subprocess.run(cmd, input=sql, capture_output=True, text=True)
    return result.stdout.strip()


def get_maildir_ids(user):
    """get all maildir_ids from PG for this user, ordered by uid."""
    result = remote_psql(f"""
        SELECT msg.maildir_id FROM messages msg
        JOIN mailboxes mb ON msg.mailbox_id = mb.id
        WHERE mb.user_address = '{user}'
        ORDER BY mb.name, msg.uid;
    """)
    return [line.strip() for line in result.split('\n') if line.strip()]


def main():
    emails_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else None
    if not emails_dir or not emails_dir.is_dir():
        print("Usage: python3 fix-maildir.py /path/to/emails")
        sys.exit(1)

    for account_dir in sorted(emails_dir.iterdir()):
        if not account_dir.is_dir() or "@" not in account_dir.name:
            continue

        user = account_dir.name
        local, domain = user.split("@")

        # collect all eml files in order
        all_emls = []
        for folder_dir in sorted(account_dir.iterdir()):
            if not folder_dir.is_dir():
                continue
            for eml_path in sorted(folder_dir.glob("*.eml")):
                all_emls.append(eml_path)

        if not all_emls:
            continue

        # get maildir_ids from PG (these are what we generated during import)
        maildir_ids = get_maildir_ids(user)
        if not maildir_ids:
            print(f"  {user}: no maildir_ids in PG, skipping")
            continue

        # sort emls by date to match import order
        def parse_date(path):
            data = path.read_bytes()
            target = b"date:"
            for line in data.split(b"\n"):
                s = line.rstrip(b"\r")
                if s == b"":
                    break
                if s.lower().startswith(target):
                    try:
                        return int(email.utils.parsedate_to_datetime(
                            s[len(target):].decode("utf-8", errors="replace").strip()
                        ).timestamp())
                    except Exception:
                        pass
            return int(time.time())

        # sort same way as import: by folder then by date within sorted folders
        folder_emls = {}
        for eml_path in all_emls:
            folder = eml_path.parent.name
            folder_emls.setdefault(folder, []).append(eml_path)

        sorted_emls = []
        for folder in sorted(folder_emls.keys()):
            emls = folder_emls[folder]
            emls_with_date = [(p, parse_date(p)) for p in emls]
            emls_with_date.sort(key=lambda x: x[1])
            sorted_emls.extend([p for p, _ in emls_with_date])

        if len(sorted_emls) != len(maildir_ids):
            print(f"  {user}: mismatch - {len(sorted_emls)} files vs {len(maildir_ids)} PG records, skipping")
            continue

        print(f"  {user}: packing {len(sorted_emls)} files...")

        # create tar
        with tempfile.NamedTemporaryFile(suffix=".tar.gz", delete=False) as tmp:
            tar_path = tmp.name
        with tarfile.open(tar_path, "w:gz") as tar:
            for eml_path, maildir_id in zip(sorted_emls, maildir_ids):
                data = eml_path.read_bytes()
                filename = f"cur/{maildir_id}:2,S"
                info = tarfile.TarInfo(name=filename)
                info.size = len(data)
                tar.addfile(info, io.BytesIO(data))

        tar_mb = os.path.getsize(tar_path) / (1024 * 1024)
        print(f"    tar: {tar_mb:.1f} MB, uploading...")

        remote_tar = f"/tmp/mailrs-fix-{local}.tar.gz"
        scp_upload(tar_path, remote_tar)
        os.unlink(tar_path)

        host_maildir = f"{HOST_VOLUME}/{domain}/{local}"
        print(f"    extracting...")
        ssh_cmd(f"mkdir -p {host_maildir}/{{tmp,new,cur}} && tar xzf {remote_tar} -C {host_maildir} && rm -f {remote_tar}")
        print(f"    done")

    # verify
    print("\nVerification:")
    for account_dir in sorted(emails_dir.iterdir()):
        if not account_dir.is_dir() or "@" not in account_dir.name:
            continue
        user = account_dir.name
        local, domain = user.split("@")
        count = ssh_cmd(f"ls {HOST_VOLUME}/{domain}/{local}/cur/ 2>/dev/null | wc -l").strip()
        if count and count != "0":
            print(f"  {user}: {count} maildir files")


if __name__ == "__main__":
    main()
