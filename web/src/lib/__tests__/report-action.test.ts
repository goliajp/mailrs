import type { ThreadMessage } from '@/lib/types'
import type { WireReportOutcome } from '@/wire/schemas/report'

import { describe, expect, it } from 'vitest'

import { outcomeText, reportedMessage } from '@/hooks/use-report-mutation'
import { ACTIONS, rowActions } from '@/lib/actions'
import { wireReportOutcomeSchema } from '@/wire/schemas/report'

const msg = (message_id: string, sender: string, internal_date: number) =>
  ({ internal_date, message_id, sender }) as ThreadMessage

const outcome = (o: Partial<WireReportOutcome>): WireReportOutcome => ({
  at: 1,
  decided_at: 2,
  grounds: [],
  inboxes: 0,
  message_id: 'm',
  report_id: 'rp1',
  state: 'none_found',
  taken_back: 0,
  thread_id: 't',
  waits: [],
  ...o,
})

describe('reporting a scam', () => {
  it('names the newest message somebody else wrote', () => {
    const thread = [
      msg('a@x', 'Desk <notice@desk.example>', 1),
      msg('mine@me', 'Alice <alice@example.test>', 3),
      msg('b@x', 'Desk <notice@desk.example>', 2),
    ]
    expect(reportedMessage(thread, 'alice@example.test')).toBe('b@x')
    expect(
      reportedMessage([msg('mine@me', 'alice@example.test', 1)], 'alice@example.test')
    ).toBeNull()
  })

  it('says what became of the other copies, in each state', () => {
    expect(outcomeText(outcome({ state: 'taken_back', taken_back: 3 }))).toContain('3')
    const texts = (['taken_back', 'none_found', 'waiting', 'dismissed'] as const).map((state) =>
      outcomeText(outcome({ state }))
    )
    expect(new Set(texts).size).toBe(4)
  })

  it('is offered on every conversation but in the Trash, and nothing undoes it', () => {
    const base = { archived: false, flagged: false, pinned: false, snoozed: false, unread: false }
    for (const view of ['other', 'np', 'junk'] as const) {
      expect(rowActions({ ...base, view })).toContain('report')
    }
    expect(rowActions({ ...base, view: 'trash' })).not.toContain('report')
    expect(ACTIONS.report.undo).toBeNull()
    expect(ACTIONS.report.batch).toBe(false)
  })

  it('reads the server answer as the handler writes it', () => {
    const fromServer = {
      at: 1793700000,
      decided_at: 0,
      grounds: [],
      inboxes: 1,
      message_id: 's1@parcel-desk.example',
      report_id: 'rp7e14649fa2a09533',
      state: 'waiting',
      taken_back: 0,
      thread_id: 's1@parcel-desk.example',
      waits: ['no_ground'],
    }
    expect(wireReportOutcomeSchema.parse(fromServer).waits).toEqual(['no_ground'])
  })
})
