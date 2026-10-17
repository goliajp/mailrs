/**
 * DMARC wire-schema tests.
 *
 * The fixtures below are byte-identical to the JSON asserted in
 * `crates/webapi/src/handlers/dmarc.rs` — see its `report_list_wire_shape`
 * and `source_list_wire_shape` tests. Both sides are pinned to the same
 * literal, so renaming a Rust field breaks the Rust test before this
 * one can start failing silently in the browser.
 *
 * Fixtures
 * must come from the real handler shape, never from what the schema
 * happens to expect.
 */

import { describe, expect, it } from 'vitest'

import { wireDmarcReportListSchema, wireDmarcSourceListSchema } from '../schemas/dmarc'

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
