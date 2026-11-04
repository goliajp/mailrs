/**
 * Reporting a scam, and an administrator deciding on the copies that wait.
 */

import type { WirePendingReport, WireReportOutcome } from '../schemas/report'

import { wireFetch } from '../client'
import { wirePendingReportsResponseSchema, wireReportOutcomeSchema } from '../schemas/report'

/** `GET /api/admin/reports/pending` */
export async function fetchPendingReports(signal?: AbortSignal): Promise<WirePendingReport[]> {
  const raw = await wireFetch(wirePendingReportsResponseSchema, {
    path: '/admin/reports/pending',
    signal,
  })
  return raw.items
}

/** `GET /api/reports/{reportId}` — one of the reader's own reports. */
export async function fetchReportOutcome(
  reportId: string,
  signal?: AbortSignal
): Promise<WireReportOutcome> {
  return wireFetch(wireReportOutcomeSchema, {
    path: `/reports/${encodeURIComponent(reportId)}`,
    signal,
  })
}

/** `POST /api/admin/reports/{reportId}/confirm` */
export async function wireConfirmReport(reportId: string): Promise<WireReportOutcome> {
  return wireFetch(wireReportOutcomeSchema, {
    method: 'POST',
    path: `/admin/reports/${encodeURIComponent(reportId)}/confirm`,
  })
}

/** `POST /api/admin/reports/{reportId}/dismiss` */
export async function wireDismissReport(reportId: string): Promise<WireReportOutcome> {
  return wireFetch(wireReportOutcomeSchema, {
    method: 'POST',
    path: `/admin/reports/${encodeURIComponent(reportId)}/dismiss`,
  })
}

/** `POST /api/messages/{messageId}/report` */
export async function wireReportMessage(messageId: string): Promise<WireReportOutcome> {
  return wireFetch(wireReportOutcomeSchema, {
    method: 'POST',
    path: `/messages/${encodeURIComponent(messageId)}/report`,
  })
}
