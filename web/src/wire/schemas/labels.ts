import { z } from '@/lib/zod'

/** The colours a label can have; the server refuses any other. */
export const LABEL_COLORS = [
  'gray',
  'red',
  'orange',
  'yellow',
  'green',
  'teal',
  'blue',
  'purple',
] as const

export type LabelColor = (typeof LABEL_COLORS)[number]

// crates/mailbox/src/labels.rs — `Label`
export const labelSchema = z.object({
  color: z.enum(LABEL_COLORS).catch('gray'),
  created_at: z.number(),
  id: z.string(),
  name: z.string(),
})

export type WireLabel = z.infer<typeof labelSchema>

export const labelsSchema = z.array(labelSchema)
