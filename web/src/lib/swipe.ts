// What a swipe on a conversation row does. The setting is the user's, kept
// on the server (`/api/settings/swipe`) so the web and both phone apps do
// the same thing; the choices and the default are the shared action
// table's `swipe` block, which the server is checked against.
import type { ActionId } from '@/lib/actions'

import { toggle } from '@/lib/actions'

import table from './actions.json'

export type SwipeChoice = 'archive' | 'none' | 'read' | 'trash'

export type SwipeSetting = { left: SwipeChoice; right: SwipeChoice }

export const SWIPE_CHOICES = table.swipe.choices as SwipeChoice[]

export const DEFAULT_SWIPE = table.swipe.default as SwipeSetting

/**
 * The action a swipe set to `choice` runs on `row`, or null for none.
 *
 * In the Trash archive and trash both restore: a swipe never deletes,
 * because deleting for good is the one thing nothing takes back.
 */
export function swipeAction(
  choice: SwipeChoice,
  row: { archived: boolean; trashed_at?: number; unread_count: number }
): ActionId | null {
  if (choice === 'none') return null
  if (choice === 'read') return toggle(row.unread_count > 0, 'read', 'unread')
  if ((row.trashed_at ?? 0) > 0) return 'untrash'
  if (choice === 'trash') return 'trash'
  return toggle(row.archived, 'unarchive', 'archive')
}
