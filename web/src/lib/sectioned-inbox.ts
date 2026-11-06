// Whether the Inbox is shown by section: the user's setting on the server
// (`/api/settings/sectioned-inbox`), so a choice made here follows them to
// the phone apps. It used to live in this browser's local storage only.

import type { WireSectionedInbox } from '@/wire/schemas/mail-settings'

import { wireGetSectionedInbox, wireSetSectionedInbox } from '@/wire/endpoints/mail-settings'

/** Where this browser kept the switch before it moved to the server. */
export const SECTIONED_LOCAL_KEY = 'mailrs_sectioned_inbox'

export const sectionedInboxKey = ['settings', 'sectioned-inbox'] as const

/**
 * The server's setting. A choice still kept in this browser is written to
 * the server first, once, and dropped here after the server took it, so
 * nobody loses a choice they made. Nothing kept here: the server's value.
 */
export async function fetchSectionedInbox(signal?: AbortSignal): Promise<WireSectionedInbox> {
  const chosen = localSectionedChoice()
  if (chosen === null) return wireGetSectionedInbox(signal)
  const saved = await wireSetSectionedInbox({ enabled: chosen })
  localStorage.removeItem(SECTIONED_LOCAL_KEY)
  return saved
}

/**
 * What the reader chose in this browser before the move, or null when they
 * never touched the switch (the old switch wrote only `true` / `false`).
 */
export function localSectionedChoice(): boolean | null {
  const v = localStorage.getItem(SECTIONED_LOCAL_KEY)
  if (v === 'true') return true
  if (v === 'false') return false
  return null
}

/** Save the switch; it replaces any choice still waiting to move. */
export async function saveSectionedInbox(enabled: boolean): Promise<WireSectionedInbox> {
  const saved = await wireSetSectionedInbox({ enabled })
  localStorage.removeItem(SECTIONED_LOCAL_KEY)
  return saved
}

/**
 * Shown by section or not: the server's value once it answered; until then
 * the choice still waiting to move, else on (the server's default).
 */
export function sectionedOf(data: null | undefined | WireSectionedInbox): boolean {
  if (data) return data.enabled
  return localSectionedChoice() ?? true
}
