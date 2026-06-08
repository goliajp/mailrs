import { postJson } from '@/lib/api'

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
  scheduledAt?: string // ISO
  subject: string
  to: string[]
  token: string
}

export type SendResult = { message?: string; message_id?: string; success: boolean }

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
    if (p.scheduledAt) payload['scheduled_at'] = p.scheduledAt
    if (p.forwardMessageId) payload['forward_message_id'] = p.forwardMessageId
    if (p.forwardAttachmentsFrom) payload['forward_attachments_from'] = p.forwardAttachmentsFrom
    return postJson<SendResult>('/mail/send', payload)
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
  if (p.scheduledAt) fd.append('scheduled_at', p.scheduledAt)
  if (p.forwardMessageId) fd.append('forward_message_id', p.forwardMessageId)
  if (p.forwardAttachmentsFrom) {
    fd.append('forward_attachments_from', String(p.forwardAttachmentsFrom))
  }

  const res = await fetch('/api/mail/send-multipart', {
    body: fd,
    headers: { Authorization: `Bearer ${p.token}` },
    method: 'POST',
  })
  if (!res.ok) return { message: `Send failed (${res.status})`, success: false }
  return (await res.json()) as SendResult
}
