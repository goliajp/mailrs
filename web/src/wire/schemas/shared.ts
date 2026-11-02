import { z } from '@/lib/zod'

// Backend: crates/webapi/src/handlers/shared_inbox/*.rs

export const presenceStateSchema = z.enum(['viewing', 'composing'])
export type PresenceState = z.infer<typeof presenceStateSchema>

export const draftPointerSchema = z.object({
  author: z.string(),
  draft_id: z.number().int(),
  updated_at: z.number().int(),
})
export type WireDraftPointer = z.infer<typeof draftPointerSchema>

export const handoffSchema = z.object({
  at: z.number().int(),
  draft_id: z.number().int(),
  from: z.string(),
  from_draft_id: z.number().int(),
  to: z.string(),
})
export type WireHandoff = z.infer<typeof handoffSchema>

// handlers/shared_inbox/mod.rs — Overview
export const sharedOverviewSchema = z.object({
  assignee: z.string().nullable(),
  assignments: z.array(
    z.object({ at: z.number().int(), by: z.string(), to: z.string().nullable() })
  ),
  conv: z.number().int(),
  draft: draftPointerSchema.nullable(),
  gid: z.number().int(),
  handoffs: z.array(handoffSchema),
  members: z.array(z.string()),
  present: z.array(z.object({ state: presenceStateSchema, user: z.string() })),
  reply: z.object({ at: z.number().int(), by: z.string(), seq: z.number().int().min(0) }),
})
export type WireSharedOverview = z.infer<typeof sharedOverviewSchema>

// core-api method/shared.rs — AssignResponse
export const assignResultSchema = z.object({
  assignee: z.string().nullable(),
  rows: z.number().int().min(0),
})

// handlers/shared_inbox/notes.rs — Note
export const noteSchema = z.object({
  at: z.number().int(),
  by: z.string(),
  edited_at: z.number().int().nullable(),
  id: z.number().int(),
  text: z.string(),
})
export type WireNote = z.infer<typeof noteSchema>
export const notesSchema = z.array(noteSchema)
export const noteAddedSchema = z.object({ id: z.number().int() })

// handlers/shared_inbox/notes.rs — DeletedNote, the audited view
export const deletedNoteSchema = z.object({
  by: z.string(),
  deleted_at: z.number().int(),
  deleted_by: z.string(),
  id: z.number().int(),
  versions: z.array(z.string()),
})
export type WireDeletedNote = z.infer<typeof deletedNoteSchema>

// handlers/shared_inbox/drafts.rs — Current
export const sharedDraftSchema = z.object({
  draft: z
    .object({ body: z.string(), subject: z.string(), to: z.string() })
    .passthrough()
    .nullable(),
  pointer: draftPointerSchema,
})
export type WireSharedDraft = z.infer<typeof sharedDraftSchema>
