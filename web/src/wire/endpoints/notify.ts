import type { z } from '@/lib/zod'

import { wireFetch } from '../client'
import { emptyResponseSchema } from '../schemas/mutations'
import {
  importantSchema,
  notifyOverridesSchema,
  notifyPrefsSchema,
  notifyStatsSchema,
  type WireNotifyPrefs,
  type WireNotifyStats,
} from '../schemas/notify'

export function wireImportantContacts(): Promise<string[]> {
  return wireFetch(importantSchema, { path: '/contacts/important' })
}

export function wireNotifyOverrides(): Promise<z.infer<typeof notifyOverridesSchema>> {
  return wireFetch(notifyOverridesSchema, { path: '/notify/overrides' })
}

export function wireNotifyPrefs(): Promise<WireNotifyPrefs> {
  return wireFetch(notifyPrefsSchema, { path: '/notify' })
}

/** `always`, `never`, or null to remove. */
export async function wireNotifySender(
  address: string,
  value: 'always' | 'never' | null
): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { value },
    method: 'PUT',
    path: `/notify/senders/${encodeURIComponent(address)}`,
  })
}

export function wireNotifyStats(days = 7): Promise<WireNotifyStats> {
  return wireFetch(notifyStatsSchema, { path: `/notify/stats?days=${days}` })
}

/** `follow`, `mute`, or null to remove. */
export async function wireNotifyThread(
  threadId: string,
  value: 'follow' | 'mute' | null
): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { value },
    method: 'PUT',
    path: `/notify/threads/${encodeURIComponent(threadId)}`,
  })
}

export async function wirePutNotifyPrefs(prefs: WireNotifyPrefs): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: prefs,
    method: 'PUT',
    path: '/notify',
  })
}

export async function wireSetImportant(address: string, important: boolean): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { important },
    method: 'PUT',
    path: `/contacts/important/${encodeURIComponent(address)}`,
  })
}
