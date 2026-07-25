/**
 * DMARC wire-schema tests.
 *
 * The fixtures below are byte-identical to the JSON asserted in
 * `crates/webapi/src/handlers/dmarc.rs` — see its `report_list_wire_shape`,
 * `report_row_wire_shape_keeps_null_envelope_from`, and
 * `source_list_wire_shape` tests. Both sides are pinned to the same
 * literal, so renaming a Rust field breaks the Rust test before this
 * one can start failing silently in the browser.
 *
 * Per `.claude/rules/frontend/wire-schema-verification.md`: fixtures
 * must come from the real handler shape, never from what the schema
 * happens to expect.
 */

import { describe, expect, it } from 'vitest'

import {
  wireDmarcReportDetailSchema,
  wireDmarcReportListSchema,
  wireDmarcRowSchema,
  wireDmarcSourceListSchema,
} from '../schemas/dmarc'

const REPORT_JSON = {
  begin: 1784937600,
  email: 'noreply-dmarc-support@google.com',
  end: 1785023999,
  org_name: 'google.com',
  p: 'quarantine',
  passing: 5,
  policy_domain: 'golia.jp',
  rows: 2,
  sid: 'google.com!1234567890',
  total: 7,
}

const ROW_JSON = {
  count: 5,
  disposition: 'none',
  dkim: 'pass',
  envelope_from: null,
  header_from: 'golia.jp',
  passed: true,
  source_ip: '203.0.113.10',
  spf: 'pass',
}

describe('wireDmarcReportListSchema', () => {
  it('parses the handler shape and maps to camelCase', () => {
    const parsed = wireDmarcReportListSchema.parse({ items: [REPORT_JSON] })

    expect(parsed.items).toHaveLength(1)
    expect(parsed.items[0]).toEqual({
      begin: 1784937600,
      email: 'noreply-dmarc-support@google.com',
      end: 1785023999,
      orgName: 'google.com',
      passing: 5,
      policy: 'quarantine',
      policyDomain: 'golia.jp',
      rows: 2,
      sid: 'google.com!1234567890',
      total: 7,
    })
  })

  it('accepts an empty report list', () => {
    expect(wireDmarcReportListSchema.parse({ items: [] }).items).toEqual([])
  })

  it('rejects a missing envelope', () => {
    expect(() => wireDmarcReportListSchema.parse([REPORT_JSON])).toThrow()
  })

  it('rejects counts sent as strings', () => {
    // The handler serializes u64 as a JSON number. If that ever changes
    // this test is the tripwire.
    expect(() =>
      wireDmarcReportListSchema.parse({ items: [{ ...REPORT_JSON, total: '7' }] })
    ).toThrow()
  })
})

describe('wireDmarcRowSchema', () => {
  it('parses a row with a null envelope_from', () => {
    const parsed = wireDmarcRowSchema.parse(ROW_JSON)

    expect(parsed).toEqual({
      count: 5,
      disposition: 'none',
      dkim: 'pass',
      envelopeFrom: null,
      headerFrom: 'golia.jp',
      passed: true,
      sourceIp: '203.0.113.10',
      spf: 'pass',
    })
  })

  it('parses a row with a populated envelope_from', () => {
    const parsed = wireDmarcRowSchema.parse({ ...ROW_JSON, envelope_from: 'golia.jp' })
    expect(parsed.envelopeFrom).toBe('golia.jp')
  })

  it('rejects a row missing envelope_from entirely', () => {
    // nullable, not optional — the handler always emits the key.
    const { envelope_from: _omitted, ...withoutKey } = ROW_JSON
    expect(() => wireDmarcRowSchema.parse(withoutKey)).toThrow()
  })
})

describe('wireDmarcReportDetailSchema', () => {
  it('parses report plus rows', () => {
    const parsed = wireDmarcReportDetailSchema.parse({
      report: REPORT_JSON,
      rows: [ROW_JSON],
    })

    expect(parsed.report.sid).toBe('google.com!1234567890')
    expect(parsed.rows).toHaveLength(1)
    expect(parsed.rows[0].sourceIp).toBe('203.0.113.10')
  })
})

describe('wireDmarcSourceListSchema', () => {
  it('parses the rollup shape', () => {
    const parsed = wireDmarcSourceListSchema.parse({
      items: [
        {
          domains: ['golia.jp'],
          passing: 8,
          source_ip: '203.0.113.10',
          total: 8,
        },
      ],
      passing: 8,
      reports: 2,
      total: 8,
    })

    expect(parsed.total).toBe(8)
    expect(parsed.reports).toBe(2)
    expect(parsed.items[0]).toEqual({
      domains: ['golia.jp'],
      passing: 8,
      sourceIp: '203.0.113.10',
      total: 8,
    })
  })

  it('accepts an empty rollup', () => {
    const parsed = wireDmarcSourceListSchema.parse({
      items: [],
      passing: 0,
      reports: 0,
      total: 0,
    })
    expect(parsed.items).toEqual([])
  })
})
