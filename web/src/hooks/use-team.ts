import type { PresenceState } from '@/wire/schemas/shared'

import { useQuery } from '@tanstack/react-query'
import { atom } from 'jotai'
import { useEffect } from 'react'

import { findConversation } from '@/lib/mail-cache'
import { teamKeys } from '@/lib/team-events'
import {
  parseTeamConv,
  type TeamConv,
  wireListNotes,
  wireSharedHeartbeat,
  wireSharedLeave,
  wireSharedOverview,
} from '@/wire/endpoints/shared'

/** How often an open conversation says so; the server forgets it after 60 s. */
export const HEARTBEAT_MS = 20_000

/** The team conversation `threadId` is the reader's copy of, if any. */
export function teamConvOf(threadId: null | string | undefined): null | TeamConv {
  if (!threadId) return null
  return parseTeamConv(findConversation(threadId)?.shared_conv)
}

/**
 * Say, every {@link HEARTBEAT_MS}, that the reader has the conversation
 * open (or is writing a reply), and say they left when it closes. Only a
 * hint for the rest of the team: nothing is locked.
 */
export function usePresence(c: null | TeamConv, state: PresenceState) {
  const gid = c?.gid
  const conv = c?.conv
  // leaving is said once, when the conversation closes, not when the state changes
  useEffect(() => {
    if (gid === undefined || conv === undefined) return
    return () => void wireSharedLeave({ conv, gid }).catch(() => {})
  }, [gid, conv])
  useEffect(() => {
    if (gid === undefined || conv === undefined) return
    const beat = () => void wireSharedHeartbeat({ conv, gid }, state).catch(() => {})
    beat()
    const timer = setInterval(beat, HEARTBEAT_MS)
    return () => clearInterval(timer)
  }, [gid, conv, state])
}

export function useTeamNotes(c: null | TeamConv) {
  return useQuery({
    enabled: c !== null,
    queryKey: teamKeys.notes(c ?? { conv: 0, gid: 0 }),
    staleTime: 15_000,
    queryFn: ({ signal }) => wireListNotes(c as TeamConv, signal),
  })
}

export function useTeamOverview(c: null | TeamConv) {
  return useQuery({
    enabled: c !== null,
    queryKey: teamKeys.overview(c ?? { conv: 0, gid: 0 }),
    staleTime: 15_000,
    queryFn: ({ signal }) => wireSharedOverview(c as TeamConv, signal),
  })
}

/** The team conversation the reader is writing a reply to, as `"{gid}:{conv}"`. */
export const teamComposingAtom = atom<null | string>(null)

export { teamKeys }
