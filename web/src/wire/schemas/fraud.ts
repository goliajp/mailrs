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
  score: z.number(),
  threshold: z.number(),
})

export const wireFraudVerdictResponseSchema = z.object({
  verdict: wireFraudVerdictSchema.nullable(),
})

export type WireFraudLayer = z.infer<typeof wireFraudLayerSchema>
export type WireFraudVerdict = z.infer<typeof wireFraudVerdictSchema>
