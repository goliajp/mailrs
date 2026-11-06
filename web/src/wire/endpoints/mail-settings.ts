/**
 * Forwarding and the vacation reply
 * (`crates/webapi/src/handlers/mail_settings.rs`). Asking to forward to a
 * new address needs a recent password confirmation (`confirm_required`,
 * which `lib/http.ts` turns into the password prompt) and sends a code to
 * that address; forwarding starts once the code is entered.
 */

import type { SwipeSetting } from '@/lib/swipe'

import { wireFetch } from '../client'
import {
  forwardingSchema,
  forwardingVerifiedSchema,
  sectionedInboxSchema,
  swipeSchema,
  vacationSchema,
  type WireSectionedInbox,
  type WireVacation,
} from '../schemas/mail-settings'
import { emptyResponseSchema } from '../schemas/mutations'

export function wireGetForwarding(signal?: AbortSignal) {
  return wireFetch(forwardingSchema, { path: '/settings/forwarding', signal })
}

export function wireGetSectionedInbox(signal?: AbortSignal) {
  return wireFetch(sectionedInboxSchema, { path: '/settings/sectioned-inbox', signal })
}

export function wireGetSwipe(signal?: AbortSignal) {
  return wireFetch(swipeSchema, { path: '/settings/swipe', signal })
}

export function wireGetVacation(signal?: AbortSignal) {
  return wireFetch(vacationSchema, { path: '/settings/vacation', signal })
}

export async function wireRequestForwarding(address: string, keepCopy: boolean): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { address, keep_copy: keepCopy },
    method: 'POST',
    path: '/settings/forwarding',
  })
}

export function wireSetSectionedInbox(s: WireSectionedInbox) {
  return wireFetch(sectionedInboxSchema, {
    body: s,
    method: 'PUT',
    path: '/settings/sectioned-inbox',
  })
}

export function wireSetSwipe(s: SwipeSetting) {
  return wireFetch(swipeSchema, { body: s, method: 'PUT', path: '/settings/swipe' })
}

export function wireSetVacation(v: Omit<WireVacation, 'version'>) {
  return wireFetch(vacationSchema, { body: v, method: 'PUT', path: '/settings/vacation' })
}

export async function wireStopForwarding(): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: '/settings/forwarding',
  })
}

export function wireVerifyForwarding(code: string) {
  return wireFetch(forwardingVerifiedSchema, {
    body: { code },
    method: 'POST',
    path: '/settings/forwarding/verify',
  })
}
