/**
 * The stored fraud verdict for one message.
 *
 * Backend: crates/webapi/src/handlers/quarantine.rs — `get_fraud_verdict`,
 * which proxies `crates/fastcore/src/routes/quarantine.rs` and returns
 * `{"verdict": <the stored JSON> | null}`. The verdict's own shape is
 * owned by `mailrs_inbound::verdict::FraudVerdict` (crates/inbound/src/
 * verdict.rs), which has the round-trip test.
 *
 * Verified 2026-11-01 against that struct's serde derive.
 *
 * `null` is the ordinary answer and means *nobody suspected this
 * message and nothing sorted it* — not *it was examined and cleared*. Two different facts,
 * and the panel must not draw them the same way.
 */

import { z } from '@/lib/zod'

import { wireReasonSchema } from './reason'

export const wireFraudLayerSchema = z.object({
  // English, for the log and for a code this client has no sentence for
  detail: z.string(),
  name: z.string(),
  outcome: z.enum(['pass', 'fail', 'not_applicable']),
  // absent on verdicts stored before reasons existed
  reasons: z.array(wireReasonSchema).default([]),
  score: z.number(),
})

export const wireFraudVerdictSchema = z.object({
  // empty on a record that exists only to say why mail nobody suspected
  // was filed where it was
  layers: z.array(wireFraudLayerSchema),
  // why it was filed where it was: Junk, a sender rule, the classifier,
  // a filter; left out when empty
  placement: z.array(wireReasonSchema).default([]),
  quarantined: z.boolean(),
  rules_version: z.string(),
  // Reported, not decided on. What holds a conversation is any fraud
  // finding at all (`mailrs_inbound::verdict::holds`); a threshold
  // beside a score that does not reach it read as a contradiction on
  // a screen already showing the conversation as held, and on
  // production it was one for 43 of 51.
  score: z.number(),
})

export const wireFraudVerdictResponseSchema = z.object({
  verdict: wireFraudVerdictSchema.nullable(),
})

export type WireFraudLayer = z.infer<typeof wireFraudLayerSchema>
export type WireFraudVerdict = z.infer<typeof wireFraudVerdictSchema>
