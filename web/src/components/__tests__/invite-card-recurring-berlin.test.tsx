import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { getDefaultStore } from 'jotai'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { InviteCard } from '@/components/invite-card'
import { chosenLangAtom } from '@/i18n'

import { recurringDetail } from './recurring-invite'

// A reader whose own clocks change too, a week before the US ones: Berlin
// goes back on 25 October, Santa Clara on 1 November. In the week between,
// the same 22:00 Pacific meeting is 06:00 in Berlin; after it, 07:00. Each
// occurrence is converted with the reader's offset at that instant.
vi.hoisted(() => {
  process.env.TZ = 'Europe/Berlin'
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

describe('a weekly invitation read in Berlin', () => {
  it('is at 06:00 in the week between the two changes', async () => {
    get.mockResolvedValue(recurringDetail('before-change'))
    card()
    const line = (await screen.findByText(/下一次/)).parentElement
    expect(line?.textContent).toContain('10月30日 06:00')
    expect(line?.textContent).toContain('22:00 PDT')
  })

  it('is at 07:00 once both have changed', async () => {
    get.mockResolvedValue(recurringDetail('after-change'))
    card()
    const line = (await screen.findByText(/下一次/)).parentElement
    expect(line?.textContent).toContain('11月6日 07:00')
    expect(line?.textContent).toContain('22:00 PST')
  })
})
