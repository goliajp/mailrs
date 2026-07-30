import { wireSendMailJson, wireSendMailMultipart } from '@/wire/endpoints/mail'
import { WireErrorException } from '@/wire/errors'

export type SendMailParams = {
  attachments?: File[]
  bcc?: string[]
  body: string
  cc?: string[]
  forwardAttachmentsFrom?: null | number
  forwardMessageId?: null | string
  from: string
  htmlBody: string
  inReplyTo?: null | string
  /** Which carried attachments to keep, as indices into the original
   *  envelope. `null` keeps all; `[]` keeps none — the two are different
   *  and the wire preserves the difference. */
  redraftKeep?: null | number[]
  /** The send being repaired; its attachments are carried server-side. */
  redraftOf?: null | string
  /**
   * The conversation this reply belongs to.
   *
   * A threading fallback the server uses when `inReplyTo` is missing: it
   * resolves the parent's Message-ID from the thread. A reply with an
   * attachment went out unthreaded on 2026-07-30 while attachment-less ones
   * the same day were fine, and the draft round-trip drops the parent
   * message id entirely — so every surface that can reply now sends the one
   * thing it always knows, which conversation it is in.
   */
  replyToThreadId?: null | string
  /**
   * Unix epoch **seconds**, the form the wire has always wanted
   * (`SendRequest.scheduled_at: Option<i64>`, and every MCP scheduling
   * tool documents "Unix epoch seconds").
   *
   * This was typed `string // ISO` and sent as one, which no layer
   * rejected in a useful way: the JSON path 422'd the whole request, and
   * the multipart path parsed the ISO string as an i64, failed, dropped it
   * to `None` and **sent the mail immediately** — so a mail scheduled for
   * tomorrow morning went out at once, with nothing said. A number cannot
   * be mis-serialized into an i64 field, which is why the type changed
   * rather than a conversion being added on top.
   */
  scheduledAt?: null | number
  subject: string
  to: string[]
  token: string
}

export type SendResult = { message?: string; message_id?: string; success: boolean }

/**
 * A `<input type="datetime-local">` value → Unix epoch seconds.
 *
 * The input carries no zone, so `new Date(v)` reads it as local time,
 * which is what someone picking "09:00" means.
 *
 * Returns `null` for empty or unparseable input. Not `NaN`: `NaN` survives
 * as far as `JSON.stringify`, which turns it into `null`, which the
 * backend reads as "not scheduled" and sends at once — the same silent
 * failure by another route.
 */
export function epochSecondsFromLocalInput(value: string): null | number {
  if (!value.trim()) return null
  const ms = new Date(value).getTime()
  if (!Number.isFinite(ms)) return null
  return Math.floor(ms / 1000)
}

// Comma/semicolon-separated address list → trimmed non-empty entries.
export function parseAddressList(input: string): string[] {
  return input
    .split(/[,;]/)
    .map((s) => s.trim())
    .filter(Boolean)
}

// Single send path used by both new-conversation and reply-box. Picks the
// transport (multipart for attachments, JSON otherwise) and forwards every
// optional field. Caller owns UI state (sending flag, toasts, draft save).
export async function sendMail(p: SendMailParams): Promise<SendResult> {
  const attachments = p.attachments ?? []
  if (attachments.length === 0) {
    const payload: Record<string, unknown> = {
      bcc: p.bcc ?? [],
      body: p.body,
      cc: p.cc ?? [],
      from: p.from,
      html_body: p.htmlBody,
      subject: p.subject,
      to: p.to,
    }
    if (p.inReplyTo) payload['in_reply_to'] = p.inReplyTo
    // Guarded on null, not falsiness: epoch 0 is a real instant, and
    // although nobody schedules mail for 1970 the check should not be the
    // reason it works.
    if (p.scheduledAt !== null && p.scheduledAt !== undefined) {
      payload['scheduled_at'] = p.scheduledAt
    }
    if (p.forwardMessageId) payload['forward_message_id'] = p.forwardMessageId
    if (p.forwardAttachmentsFrom) payload['forward_attachments_from'] = p.forwardAttachmentsFrom
    if (p.redraftOf) payload['redraft_of'] = p.redraftOf
    if (p.replyToThreadId) payload['reply_to_thread_id'] = p.replyToThreadId
    // Absent keeps every carried attachment; present-and-empty keeps
    // none. A falsy check here would re-attach files the user removed.
    if (p.redraftKeep !== null && p.redraftKeep !== undefined) {
      payload['redraft_keep'] = p.redraftKeep
    }
    return wireSendMailJson(payload)
  }

  const fd = new FormData()
  fd.append('from', p.from)
  fd.append('subject', p.subject)
  fd.append('body', p.body)
  fd.append('html_body', p.htmlBody)
  for (const r of p.to) fd.append('to', r)
  for (const r of p.cc ?? []) fd.append('cc', r)
  for (const r of p.bcc ?? []) fd.append('bcc', r)
  for (const f of attachments) fd.append('attachments', f)
  if (p.inReplyTo) fd.append('in_reply_to', p.inReplyTo)
  if (p.scheduledAt !== null && p.scheduledAt !== undefined) {
    fd.append('scheduled_at', String(p.scheduledAt))
  }
  if (p.forwardMessageId) fd.append('forward_message_id', p.forwardMessageId)
  if (p.forwardAttachmentsFrom) {
    fd.append('forward_attachments_from', String(p.forwardAttachmentsFrom))
  }
  if (p.redraftOf) fd.append('redraft_of', p.redraftOf)
  if (p.replyToThreadId) fd.append('reply_to_thread_id', p.replyToThreadId)
  // One comma-separated field, not a repeated one: repeating it cannot
  // express "keep none", since zero occurrences and an empty selection
  // both arrive as no field at all and mean opposite things.
  if (p.redraftKeep !== null && p.redraftKeep !== undefined) {
    fd.append('redraft_keep', p.redraftKeep.join(','))
  }

  try {
    return await wireSendMailMultipart(fd)
  } catch (e) {
    if (e instanceof WireErrorException && e.detail.kind === 'server') {
      return { message: e.detail.message ?? `Send failed (${e.detail.status})`, success: false }
    }
    return { message: 'Send failed', success: false }
  }
}
