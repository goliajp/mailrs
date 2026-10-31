import { z } from '@/lib/zod'

// Backend: crates/webapi/src/handlers/screening_admin.rs — fn list
// (types: crates/reputation/src/lib.rs `Observation`,
//  crates/attachment-scan/src/lib.rs `HashObservation`)
export const observationSchema = z.object({
  actor: z.string().nullable().optional(),
  at: z.number(),
  id: z.string(),
  kind: z.string(),
  reason: z.string(),
  retracts: z.string().nullable().optional(),
  source: z.string(),
  stance: z.string(),
  value: z.string(),
})

export const hashObservationSchema = z.object({
  actor: z.string().nullable().optional(),
  at: z.number(),
  id: z.string(),
  reason: z.string(),
  retracts: z.string().nullable().optional(),
  sha256: z.string(),
  stance: z.string(),
})

export const screeningListSchema = z.object({
  hashes: z.array(hashObservationSchema),
  observations: z.array(observationSchema),
})

export const recordedSchema = z.object({ id: z.string() })

export type HashBody = {
  reason: string
  retracts?: string
  sha256: string
  stance: 'allow' | 'deny' | 'retract'
}
/** What an administrator records by hand. */
export type ObservationBody = {
  kind: 'cidr' | 'ip' | 'link_domain' | 'sender_domain'
  reason: string
  retracts?: string
  source: 'incident' | 'manual'
  stance: 'allow' | 'deny' | 'retract'
  value: string
}

export type WireHashObservation = z.infer<typeof hashObservationSchema>

export type WireObservation = z.infer<typeof observationSchema>
