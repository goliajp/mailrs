/**
 * Answering a meeting invitation.
 *
 * Backend: crates/webapi/src/handlers/invites.rs — fn submit_rsvp (RsvpRequest),
 * fn submit_counter (CounterRequest). Both answer `{success, message?}`;
 * `success: false` means the invite was not found or not answerable.
 */

import { z } from '@/lib/zod'

import { wireFetch } from '../client'

const answerSchema = z.object({ message: z.string().optional(), success: z.boolean() })

/** Counter-propose a time. Both ends are UTC ISO-8601 and both are required. */
export type CounterBody = { end: string; start: string }

export type InviteAnswer = z.infer<typeof answerSchema>

export type RsvpBody = {
  partstat: 'ACCEPTED' | 'DECLINED' | 'TENTATIVE'
  recurrence_id?: string
}

export function wireInviteCounter(uid: number | string, body: CounterBody): Promise<InviteAnswer> {
  return wireFetch(answerSchema, { body, method: 'POST', path: `/invites/${uid}/counter` })
}

export function wireInviteRsvp(uid: number | string, body: RsvpBody): Promise<InviteAnswer> {
  return wireFetch(answerSchema, { body, method: 'POST', path: `/invites/${uid}/rsvp` })
}
