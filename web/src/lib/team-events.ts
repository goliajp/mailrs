import type { TeamConv } from '@/wire/endpoints/shared'

import { queryClient } from '@/lib/query-client'
import { conversationKeys } from '@/store/query-keys-v21'

export const teamKeys = {
  all: () => ['team'] as const,
  conv: (c: TeamConv) => [...teamKeys.all(), c.gid, c.conv] as const,
  draft: (c: TeamConv) => [...teamKeys.conv(c), 'draft'] as const,
  notes: (c: TeamConv) => [...teamKeys.conv(c), 'notes'] as const,
  overview: (c: TeamConv) => [...teamKeys.conv(c), 'overview'] as const,
}

/** The team events the live connection carries; each names what changed. */
export type TeamEvent = {
  conv: number
  gid: number
  type: 'SharedAssigned' | 'SharedDraft' | 'SharedNotes' | 'SharedPresence' | 'SharedReplied'
}

export function isTeamEvent(e: { type: string }): e is TeamEvent {
  return e.type.startsWith('Shared')
}

/** Refetch what a team event says changed. */
export function onTeamEvent(e: TeamEvent) {
  const c = { conv: e.conv, gid: e.gid }
  if (e.type === 'SharedNotes') {
    void queryClient.invalidateQueries({ queryKey: teamKeys.notes(c) })
    return
  }
  void queryClient.invalidateQueries({ queryKey: teamKeys.overview(c) })
  if (e.type === 'SharedDraft') void queryClient.invalidateQueries({ queryKey: teamKeys.draft(c) })
  // the assignee is on every member's row, and "assigned to me" moves
  if (e.type === 'SharedAssigned') {
    void queryClient.invalidateQueries({ queryKey: conversationKeys.all() })
  }
}
