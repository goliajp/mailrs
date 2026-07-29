/**
 * DMARC aggregate-report wire schemas.
 *
 * Backend: crates/webapi/src/handlers/dmarc.rs
 *   - :154 pub async fn list_reports  -> Json<ReportListResponse>
 *   - :181 pub async fn get_report    -> Json<ReportDetailResponse>
 *   - :204 pub async fn list_sources  -> Json<SourceListResponse>
 *
 * Rust structs these mirror (same file):
 *   - :67  ReportSummary { sid, org_name, email, policy_domain,
 *                          begin, end, p, total, passing, rows }
 *   - :99  ReportRow     { source_ip, count, disposition, dkim, spf,
 *                          header_from, envelope_from, passed }
 *   - :129 SourceSummary { source_ip, total, passing, domains }
 *
 * Field names are snake_case on the wire because the Rust structs use
 * plain `#[derive(Serialize)]` with no rename attribute. These schemas
 * transform to camelCase for UI use, so the boundary is the only place
 * that knows the wire casing.
 *
 * Numeric fields are `u64` / `i64` in Rust and arrive as JSON numbers.
 * They are typed as numbers here rather than coerced from strings —
 * verified against the handler, which serializes them as integers.
 */

import { z } from 'zod'

/** One report's summary row. Mirrors Rust `ReportSummary`. */
export const wireDmarcReportSchema = z
  .object({
    begin: z.number(),
    email: z.string(),
    end: z.number(),
    org_name: z.string(),
    p: z.string(),
    passing: z.number(),
    policy_domain: z.string(),
    rows: z.number(),
    sid: z.string(),
    total: z.number(),
  })
  .transform((v) => ({
    begin: v.begin,
    email: v.email,
    end: v.end,
    orgName: v.org_name,
    passing: v.passing,
    policy: v.p,
    policyDomain: v.policy_domain,
    rows: v.rows,
    sid: v.sid,
    total: v.total,
  }))

/** A parsed report summary as the UI consumes it. */
export type DmarcReport = z.infer<typeof wireDmarcReportSchema>

/** `GET /api/admin/dmarc/reports`. Mirrors Rust `ReportListResponse`. */
export const wireDmarcReportListSchema = z.object({
  items: z.array(wireDmarcReportSchema),
})

/** One row inside a report. Mirrors Rust `ReportRow`. */
export const wireDmarcRowSchema = z
  .object({
    count: z.number(),
    disposition: z.string(),
    dkim: z.string(),
    envelope_from: z.string().nullable(),
    header_from: z.string(),
    passed: z.boolean(),
    source_ip: z.string(),
    spf: z.string(),
  })
  .transform((v) => ({
    count: v.count,
    disposition: v.disposition,
    dkim: v.dkim,
    envelopeFrom: v.envelope_from,
    headerFrom: v.header_from,
    passed: v.passed,
    sourceIp: v.source_ip,
    spf: v.spf,
  }))

/** A report row as the UI consumes it. */
export type DmarcRow = z.infer<typeof wireDmarcRowSchema>

/** `GET /api/admin/dmarc/reports/{sid}`. Mirrors `ReportDetailResponse`. */
export const wireDmarcReportDetailSchema = z.object({
  report: wireDmarcReportSchema,
  rows: z.array(wireDmarcRowSchema),
})

/** One sending source rolled up. Mirrors Rust `SourceSummary`. */
export const wireDmarcSourceSchema = z
  .object({
    domains: z.array(z.string()),
    passing: z.number(),
    source_ip: z.string(),
    total: z.number(),
  })
  .transform((v) => ({
    domains: v.domains,
    passing: v.passing,
    sourceIp: v.source_ip,
    total: v.total,
  }))

/** A rolled-up sending source as the UI consumes it. */
export type DmarcSource = z.infer<typeof wireDmarcSourceSchema>

/** `GET /api/admin/dmarc/sources`. Mirrors Rust `SourceListResponse`. */
export const wireDmarcSourceListSchema = z.object({
  items: z.array(wireDmarcSourceSchema),
  passing: z.number(),
  reports: z.number(),
  total: z.number(),
})

/** The sources rollup as the UI consumes it. */
export type DmarcSourceList = z.infer<typeof wireDmarcSourceListSchema>
