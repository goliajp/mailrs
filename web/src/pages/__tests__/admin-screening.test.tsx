// The screening page: records an observation with its reason, and offers
// to withdraw what is not withdrawn yet.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/wire/endpoints/screening', () => ({
  wireAddScreeningHash: vi.fn(),
  wireAddScreeningObservation: vi.fn(),
  wireListScreening: vi.fn(),
}))

import { AdminScreening } from '@/pages/admin-screening'
import { wireAddScreeningObservation, wireListScreening } from '@/wire/endpoints/screening'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

const DENY = {
  actor: 'admin@golia.jp',
  at: 1,
  id: 'r1',
  kind: 'ip',
  reason: '10-24 incident',
  source: 'incident',
  stance: 'deny',
  value: '200.181.82.134',
}

function show() {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <AdminScreening />
    </QueryClientProvider>
  )
}

describe('screening', () => {
  it('lists what is recorded and records more with a reason', async () => {
    vi.mocked(wireListScreening).mockResolvedValue({ hashes: [], observations: [DENY] })
    vi.mocked(wireAddScreeningObservation).mockResolvedValue({ id: 'r2' })
    show()
    expect(await screen.findByText(/200\.181\.82\.134/)).toBeDefined()
    expect(screen.getAllByText('Withdraw')).toHaveLength(1)
    fireEvent.change(screen.getByLabelText('Value'), { target: { value: '203.0.113.9' } })
    fireEvent.change(screen.getByLabelText('Reason'), { target: { value: 'brute force' } })
    fireEvent.click(screen.getAllByText('Record')[0])
    await vi.waitFor(() =>
      expect(wireAddScreeningObservation).toHaveBeenCalledWith({
        kind: 'ip',
        reason: 'brute force',
        source: 'incident',
        stance: 'deny',
        value: '203.0.113.9',
      })
    )
  })

  it('does not offer to withdraw what is already withdrawn', async () => {
    const retract = { ...DENY, id: 'r2', retracts: 'r1', stance: 'retract', value: '' }
    vi.mocked(wireListScreening).mockResolvedValue({ hashes: [], observations: [retract, DENY] })
    show()
    expect(await screen.findByText(/withdraws r1/)).toBeDefined()
    expect(screen.queryByText('Withdraw')).toBeNull()
  })
})
