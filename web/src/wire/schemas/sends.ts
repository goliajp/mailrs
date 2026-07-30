/**
 * The Send list — one row per send, with delivery status.
 *
 * Backend: crates/webapi/src/handlers/sends.rs
 *   GET  /api/mail/sends?limit=&offset=&status=   → Vec<SendResponse>
 *   GET  /api/mail/sends/{send_id}:redraft        → RedraftResponse
 *   POST /api/mail/sends/{send_id}:resend         → ResendResponse
 *
 * `SendResponse` / `RecipientResponse` verified field-by-field against a
 * response captured from prod 2.18.14 on 2026-07-30 — see the fixture in
 * `__tests__/sends.test.ts`, which is that capture verbatim rather than a
 * shape written to match this file. Nine schemas drifted from their
 * handlers on 2026-07-08 precisely because their fixtures were written
 * from the schema, so both agreed and both were wrong.
 *
 * `:redraft` has no capture: it ships in the same commit as this file and
 * is not deployed yet. Its fields are read off the `RedraftResponse`
 * struct, and that is all the confidence they carry until one is caught.
 */

import { z } from 'zod'

/** Delivery state of one send. Mirrors `families::send::Status`. */
export const sendStatusSchema = z.enum(['scheduled', 'sending', 'delivered', 'failed', 'partial'])

export type WireSendStatus = z.infer<typeof sendStatusSchema>

/**
 * One recipient's outcome. `code` is the remote's SMTP reply, 0 before
 * any server answered — which is why `pending` is its own field rather
 * than inferred from a zero code.
 */
export const sendRecipientSchema = z.object({
  code: z.number().default(0),
  delivered: z.boolean().default(false),
  message: z.string().default(''),
  pending: z.boolean().default(false),
  recipient: z.string().default(''),
})

export type WireSendRecipient = z.infer<typeof sendRecipientSchema>

export const sendSchema = z.object({
  /** Whether resend / re-edit have envelope bytes to work from. False
   *  when the maildir write failed at send time, in which case both
   *  controls must stay hidden rather than do nothing when clicked. */
  can_resend: z.boolean().default(false),
  created_at: z.number().default(0),
  recipients: z.array(sendRecipientSchema).default([]),
  /** Set when this send repeats an earlier one. */
  resent_from: z.string().nullish(),
  send_id: z.string(),
  status: sendStatusSchema,
  subject: z.string().default(''),
  thread_id: z.string().default(''),
  /** Header form, display name included: prod stores
   *  `GOLIA <goliaaccess@gmail.com>`, not a bare address. */
  to: z.array(z.string()).default([]),
})

export type WireSend = z.infer<typeof sendSchema>

export const sendsSchema = z.array(sendSchema)

/** An attachment carried by a re-edit — described, never transferred. */
export const redraftAttachmentSchema = z.object({
  content_type: z.string().default('application/octet-stream'),
  filename: z.string().default('attachment'),
  /** Position in the original envelope. What a later send passes back in
   *  `redraft_keep`; not the filename, since two parts can share one. */
  index: z.number(),
  size: z.number().default(0),
})

export type WireRedraftAttachment = z.infer<typeof redraftAttachmentSchema>

export const redraftSchema = z.object({
  attachments: z.array(redraftAttachmentSchema).default([]),
  bcc: z.array(z.string()).default([]),
  body: z.string().default(''),
  cc: z.array(z.string()).default([]),
  html_body: z.string().default(''),
  in_reply_to: z.string().nullish(),
  redraft_of: z.string(),
  subject: z.string().default(''),
  to: z.array(z.string()).default([]),
})

export type WireRedraft = z.infer<typeof redraftSchema>

export const resendResultSchema = z.object({
  resent_from: z.string(),
  send_id: z.string(),
})

export type WireResendResult = z.infer<typeof resendResultSchema>
