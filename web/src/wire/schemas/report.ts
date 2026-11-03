/**
 * A scam report and what came of it.
 *
 * Backend: crates/webapi/src/handlers/scam_report.rs — `report`, `outcome`,
 * `pending`, `confirm`, `dismiss`; each returns the core's
 * `mailrs_core_api::method::report::ReportOutcomeWire` (or
 * `PendingReportsResponse`) as it came, built by
 * `mailrs_core_sidestate::families::reports::{to_wire, pending_to_wire}`.
 */

import { z } from '@/lib/zod'

export const wireReportOutcomeSchema = z.object({
  at: z.number(),
  decided_at: z.number(),
  /** `administrator`, `two_people`, `hold_rule`, `confirmed`, `report:<id>` */
  grounds: z.array(z.string()),
  /** Other inboxes found holding a copy. */
  inboxes: z.number(),
  message_id: z.string(),
  report_id: z.string(),
  state: z.enum(['taken_back', 'none_found', 'waiting', 'dismissed']),
  /** Conversations taken back from other inboxes. */
  taken_back: z.number(),
  thread_id: z.string(),
  /** `no_ground`, `many_inboxes`, `long_history`, `allow_listed` */
  waits: z.array(z.string()),
})

export type WireReportOutcome = z.infer<typeof wireReportOutcomeSchema>

export const wirePendingReportSchema = z.object({
  from: z.string(),
  outcome: wireReportOutcomeSchema,
  reporter: z.string(),
  samples: z.array(z.object({ from: z.string(), subject: z.string(), user: z.string() })),
  sender_domain: z.string(),
  subject: z.string(),
})

export type WirePendingReport = z.infer<typeof wirePendingReportSchema>

export const wirePendingReportsResponseSchema = z.object({
  items: z.array(wirePendingReportSchema),
})
