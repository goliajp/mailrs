/**
 * Team inboxes (`crates/webapi/src/handlers/shared_inbox/`): a team
 * conversation's state, assignment, presence, internal notes and the reply
 * being written. Every call is for current members of the shared group.
 */

import { wireFetch } from '../client'
import { emptyResponseSchema } from '../schemas/mutations'
import {
  assignResultSchema,
  deletedNoteSchema,
  draftPointerSchema,
  handoffSchema,
  noteAddedSchema,
  notesSchema,
  type PresenceState,
  sharedDraftSchema,
  sharedOverviewSchema,
  type WireDeletedNote,
  type WireDraftPointer,
  type WireHandoff,
  type WireNote,
  type WireSharedDraft,
  type WireSharedOverview,
} from '../schemas/shared'

/** A team conversation, as `"{gid}:{conv}"` names it on a member's row. */
export type TeamConv = { conv: number; gid: number }

/** The row's `shared_conv`, read back; null when it is not a team copy. */
export function parseTeamConv(tag: string | undefined): null | TeamConv {
  const m = /^(\d+):(\d+)$/.exec(tag ?? '')
  if (!m) return null
  return { conv: Number(m[2]), gid: Number(m[1]) }
}

export function wireAddNote(c: TeamConv, text: string) {
  return wireFetch(noteAddedSchema, {
    body: { text },
    method: 'POST',
    path: `/shared/${c.gid}/${c.conv}/notes`,
  })
}

/** Say the caller is writing the reply, saved as their draft `draftId`. */
export function wireAnnounceDraft(c: TeamConv, draftId: number): Promise<WireDraftPointer> {
  return wireFetch(draftPointerSchema, {
    body: { draft_id: draftId },
    method: 'PUT',
    path: `/shared/${c.gid}/${c.conv}/draft`,
  })
}

/** Assign the conversation to `to`; null takes the assignment back. */
export function wireAssignShared(c: TeamConv, to: null | string) {
  return wireFetch(assignResultSchema, {
    body: { to },
    method: 'POST',
    path: `/shared/${c.gid}/${c.conv}/assign`,
  })
}

export async function wireClearSharedDraft(c: TeamConv): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: `/shared/${c.gid}/${c.conv}/draft`,
  })
}

export async function wireDeleteNote(c: TeamConv, id: number): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: `/shared/${c.gid}/${c.conv}/notes/${id}`,
  })
}

export async function wireEditNote(c: TeamConv, id: number, text: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { text },
    method: 'PUT',
    path: `/shared/${c.gid}/${c.conv}/notes/${id}`,
  })
}

export function wireListNotes(c: TeamConv, signal?: AbortSignal): Promise<WireNote[]> {
  return wireFetch(notesSchema, { path: `/shared/${c.gid}/${c.conv}/notes`, signal })
}

/** What a deleted note said: super administrators only, and each read is audited. */
export function wireReadDeletedNote(c: TeamConv, id: number): Promise<WireDeletedNote> {
  return wireFetch(deletedNoteSchema, {
    path: `/admin/shared/${c.gid}/${c.conv}/deleted-notes/${id}`,
  })
}

/** Make an email group a team inbox, or a plain distribution list again. */
export async function wireSetGroupShared(id: number, shared: boolean): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { shared },
    method: 'PUT',
    path: `/admin/email-groups/${id}/shared`,
  })
}

export function wireSharedDraft(c: TeamConv, signal?: AbortSignal): Promise<WireSharedDraft> {
  return wireFetch(sharedDraftSchema, { path: `/shared/${c.gid}/${c.conv}/draft`, signal })
}

export async function wireSharedHeartbeat(c: TeamConv, state: PresenceState): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { state },
    method: 'POST',
    path: `/shared/${c.gid}/${c.conv}/presence`,
  })
}

export async function wireSharedLeave(c: TeamConv): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: `/shared/${c.gid}/${c.conv}/presence`,
  })
}

export function wireSharedOverview(c: TeamConv, signal?: AbortSignal): Promise<WireSharedOverview> {
  return wireFetch(sharedOverviewSchema, { path: `/shared/${c.gid}/${c.conv}`, signal })
}

export function wireTakeOverDraft(c: TeamConv): Promise<WireHandoff> {
  return wireFetch(handoffSchema, {
    method: 'POST',
    path: `/shared/${c.gid}/${c.conv}/draft/take-over`,
  })
}
