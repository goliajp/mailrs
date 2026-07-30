import type { WireSentMessage } from '@/wire/schemas/mail'
import type { WireSend, WireSendStatus } from '@/wire/schemas/sends'

/**
 * The Send view is a **full outer join** of two sources. Neither is
 * complete on its own.
 *
 * *The sent axis* (`mailrs:user:<u>:threads:sent`) holds every outbound
 * message ever — but **nothing on the ingest path writes it**. Its only
 * writer is fastcore's periodic maildir sweep, which scans for files whose
 * `From:` is the mailbox owner. So a send is invisible here until the sweep
 * next runs, and that sweep backs off exponentially while idle (the fix for
 * the 2026-07-19 CPU incident), so the wait can be long. This is what the
 * original "sent 里找不到, 1 分多钟后才出现" report was, and a reply to
 * nagata@nagatax.tokyo.jp was still missing after several minutes on
 * 2026-07-30 while the mail itself had been accepted with a 250.
 *
 * *The Send projection* is written synchronously in the fallible step of
 * the enqueue, so a send that returned 200 has a row immediately — but it
 * only covers sends made since it shipped (2026-07-30). Backfilling it
 * would have meant writing `delivered` on mail whose outcome nobody
 * recorded, some of which bounced.
 *
 * Taking either as *the* list loses something real: the axis alone hides
 * every recent send, and the projection alone hides all history. So both
 * contribute rows, deduped on `message_id`:
 *
 * | in axis | in projection | shown | badge |
 * |---|---|---|---|
 * | yes | yes | yes | from the row |
 * | yes | no | yes | none — outcome unrecorded |
 * | no | yes | **yes** | from the row |
 *
 * The third line is the one that was missing. `null` is not `0`: a send
 * with no row has an unknown outcome and the honest display of unknown is
 * nothing at all (`.claude/rules/common/coding-style.md` → Null vs Zero).
 *
 * The join key is `message_id`. Both sides store it bare, without angle
 * brackets — verified against a prod capture of each on 2026-07-30. If one
 * side ever gains brackets the join fails silently, which is why `joinKey`
 * is one function and not two inline expressions.
 */
export type SendRow = {
  /** The date the row sorts on, epoch seconds. */
  date: number
  /** Identity, and the React key. Never empty on either source. */
  messageId: string
  /** Absent for a send the sweep has not filed yet. */
  msg: null | WireSentMessage
  /** Absent for every send that predates the projection. */
  send: null | WireSend
  /** Subject and recipients, from whichever source has them. */
  subject: string
  threadId: string
  to: string
  /** The exact outbound message to focus when the thread opens, when known. */
  uid: null | number
}

/**
 * The recipients that did not make it, with what the remote said.
 *
 * Empty when everything landed — the detail panel exists to explain a
 * failure, and listing successes alongside buries the one line that
 * matters.
 */
export function failedRecipients(send: WireSend): WireSend['recipients'] {
  return send.recipients.filter((r) => !r.delivered && !r.pending)
}

/**
 * Index sends by the message they belong to, keeping the newest attempt
 * per message.
 *
 * A resend creates a second Send row for the same message. The row that
 * matters is the latest one — the question the view answers is "where
 * does this mail stand now", not "how did the first attempt go".
 */
export function indexSendsByMessage(sends: readonly WireSend[]): Map<string, WireSend> {
  const out = new Map<string, WireSend>()
  for (const send of sends) {
    // `resent_from` names the original, so its own `send_id` carries the
    // `#r<n>` suffix and cannot be the join key.
    const key = joinKey(send.resent_from ?? send.send_id)
    const seen = out.get(key)
    if (!seen || send.created_at > seen.created_at) {
      out.set(key, send)
    }
  }
  return out
}

/** Normalise a Message-ID for comparison: no brackets, no case. */
export function joinKey(raw: string): string {
  return raw.trim().replace(/^</, '').replace(/>$/, '').toLowerCase()
}

/** Attach each message's send record, where there is one. */
export function joinSends(
  messages: readonly WireSentMessage[],
  sends: readonly WireSend[]
): SendRow[] {
  const byMessage = indexSendsByMessage(sends)
  const rows = new Map<string, SendRow>()

  for (const msg of messages) {
    const key = joinKey(msg.message_id)
    if (!key) continue
    const send = byMessage.get(key) ?? null
    rows.set(key, {
      date: msg.internal_date,
      messageId: msg.message_id,
      msg,
      send,
      subject: msg.subject,
      threadId: msg.thread_id,
      to: msg.to,
      uid: msg.uid,
    })
  }

  // Sends the sweep has not filed yet. Without this pass a send that
  // succeeded — 250 from the remote, row written, mail in the maildir — is
  // absent from the only screen that would show it.
  for (const [key, send] of byMessage) {
    if (rows.has(key)) continue
    rows.set(key, {
      date: send.created_at,
      messageId: send.send_id,
      msg: null,
      send,
      subject: send.subject,
      threadId: send.thread_id,
      // Header form, joined the same way the axis stores it, so both
      // sources render through one code path.
      to: send.to.join(', '),
      // The uid belongs to the maildir copy the sweep has not indexed. Null
      // rather than 0: 0 is a real uid shape and would make the thread view
      // try to focus a message that is not there.
      uid: null,
    })
  }

  return [...rows.values()].sort((a, b) => b.date - a.date)
}

/**
 * Whether a row needs the user's attention: a send that failed, or one
 * that reached some recipients and not others.
 */
export function needsAttention(row: SendRow): boolean {
  if (!row.send) return false
  return row.send.status === 'failed' || row.send.status === 'partial'
}

const STATUS_LABELS: Record<WireSendStatus, string> = {
  delivered: 'Delivered',
  failed: 'Failed',
  partial: 'Partly delivered',
  scheduled: 'Scheduled',
  sending: 'Sending',
}

/**
 * Filter rows to one status.
 *
 * `null` shows everything. Rows with no send record are excluded by any
 * status filter rather than swept into a default bucket — they have no
 * status, and putting them under one would be a claim about their
 * outcome.
 */
export function filterByStatus(rows: readonly SendRow[], status: null | WireSendStatus) {
  if (!status) return [...rows]
  return rows.filter((row) => row.send?.status === status)
}

export function statusLabel(status: WireSendStatus): string {
  return STATUS_LABELS[status]
}
