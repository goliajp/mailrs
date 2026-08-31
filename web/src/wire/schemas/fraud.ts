/**
 * The stored fraud verdict for one message.
 *
 * Backend: crates/webapi/src/handlers/quarantine.rs — `get_fraud_verdict`,
 * which proxies `crates/fastcore/src/routes/quarantine.rs` and returns
 * `{"verdict": <the stored JSON> | null}`. The verdict's own shape is
 * owned by `mailrs_inbound::verdict::FraudVerdict` (crates/inbound/src/
 * verdict.rs), which has the round-trip test.
 *
 * Verified 2026-08-28 against that struct's serde derive.
 *
 * `null` is the ordinary answer and means *nobody suspected this
 * message* — not *it was examined and cleared*. Two different facts,
 * and the panel must not draw them the same way.
 */

import { z } from 'zod'

export const wireFraudLayerSchema = z.object({
  detail: z.string(),
  name: z.string(),
  outcome: z.enum(['pass', 'fail', 'not_applicable']),
  score: z.number(),
})

export const wireFraudVerdictSchema = z.object({
  layers: z.array(wireFraudLayerSchema),
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

/**
 * `GET /api/quarantine` — the envelope, read for its count.
 *
 * Backend: crates/webapi/src/handlers/quarantine.rs:`QuarantineListResponse`,
 * `{items: Vec<ConversationResponse>, total: usize}`. Verified
 * 2026-08-31 against that struct; it was a bare array until the same
 * day, which is why `items` is optional here — a client running
 * against an older core still parses, and reads a count of zero
 * rather than throwing.
 */
export const wireQuarantineCountSchema = z.object({
  items: z.array(z.unknown()).optional(),
  total: z.number().int().nonnegative().default(0),
})
