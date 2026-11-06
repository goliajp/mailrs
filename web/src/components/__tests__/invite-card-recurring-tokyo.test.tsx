import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { getDefaultStore } from 'jotai'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { InviteCard } from '@/components/invite-card'
import { chosenLangAtom } from '@/i18n'

import { recurringDetail } from './recurring-invite'

// The reader of the bug report: Tokyo, no daylight time. The meeting is
// 14:00 here before the US clocks go back on 1 November and 15:00 after.
vi.hoisted(() => {
  process.env.TZ = 'Asia/Tokyo'
})

const get = vi.hoisted(() => vi.fn())
vi.mock('@/wire/endpoints/admin', () => ({
  adminListGet: vi.fn(async () => []),
  adminObjectGet: get,
}))

function card() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <InviteCard messageUid={9} />
    </QueryClientProvider>
  )
}

beforeEach(() => {
  get.mockReset()
  getDefaultStore().set(chosenLangAtom, 'zh')
})

describe('a weekly invitation read in Tokyo', () => {
  it('says it repeats every Thursday until 29 April, in the organiser’s time', async () => {
    get.mockResolvedValue(recurringDetail('after-change'))
    card()
    const repeat = await screen.findByTestId('invite-repeat')
    expect(repeat.textContent).toContain('每周四')
    expect(repeat.textContent).toContain('2027年4月29日')
    expect(repeat.textContent).toContain('按组织者的时间')
  })

  it('sets the Chinese lines without Latin spacing around dates and full-width marks', async () => {
    get.mockResolvedValue(recurringDetail('after-change'))
    card()
    const repeat = await screen.findByTestId('invite-repeat')
    expect(repeat.textContent).toBe('每周四 · 到2027年4月29日为止（按组织者的时间）')
    const next = await screen.findByTestId('invite-next')
    expect(next.textContent).toMatch(/^下一次：2026/)
  })

  it('shows the next meeting at 15:00 after the US clocks go back, 22:00 PST beside it', async () => {
    get.mockResolvedValue(recurringDetail('after-change'))
    card()
    const line = (await screen.findByText(/下一次/)).parentElement
    expect(line?.textContent).toContain('11月6日 15:00')
    expect(line?.textContent).toContain('22:00 PST')
    // not the first occurrence of the series, which is already past
    expect(line?.textContent).not.toContain('10月30日')
  })

  it('shows 14:00 and 22:00 PDT for the meeting before the change', async () => {
    get.mockResolvedValue(recurringDetail('before-change'))
    card()
    const line = (await screen.findByText(/下一次/)).parentElement
    expect(line?.textContent).toContain('10月30日 14:00')
    expect(line?.textContent).toContain('22:00 PDT')
  })

  it('says the series is over once it is', async () => {
    get.mockResolvedValue(recurringDetail('ended'))
    card()
    const repeat = await screen.findByTestId('invite-repeat')
    expect(repeat.textContent).toContain('已全部结束')
    expect(screen.queryByText(/下一次/)).toBeNull()
  })

  it('labels a single invitation with the abbreviation in force on its date', async () => {
    const detail = recurringDetail('ended')
    const single = {
      ...detail,
      invite_payload: {
        ...detail.invite_payload,
        next_occurrence: undefined,
        recurrence: undefined,
      },
    }
    get.mockResolvedValue(single)
    card()
    await screen.findByText('GOLIA sync')
    expect(screen.queryByTestId('invite-repeat')).toBeNull()
    // 29 October is daylight time in Santa Clara, whatever the TZID says
    expect(document.body.textContent).toContain('22:00 PDT')
    expect(document.body.textContent).not.toContain('Pacific Standard Time')
  })
})
