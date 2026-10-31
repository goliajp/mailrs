// Rules in settings: made from a search with the actions ticked, previewed,
// and run over existing mail only after the count is confirmed.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/wire/endpoints/rules', () => ({
  wireApplyRule: vi.fn(),
  wireCreateRule: vi.fn(),
  wireDeleteRule: vi.fn(),
  wireListRules: vi.fn(),
  wirePreviewRule: vi.fn(),
  wireUndoRuleRun: vi.fn(),
  wireUpdateRule: vi.fn(),
}))
vi.mock('@/hooks/use-labels', () => ({
  useLabels: () => ({ data: [{ color: 'blue', created_at: 1, id: '3', name: 'Shopping' }] }),
}))

import { RulesSection } from '@/components/settings/rules-section'
import {
  wireApplyRule,
  wireCreateRule,
  wireListRules,
  wirePreviewRule,
} from '@/wire/endpoints/rules'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

const SHOP = {
  actions: ['archive', 'label:3'],
  created_at: 1,
  enabled: true,
  id: '1',
  name: 'Shop',
  query: 'from:shop.example',
}

function renderAt(url: string) {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter initialEntries={[url]}>
        <RulesSection />
      </MemoryRouter>
    </QueryClientProvider>
  )
}

describe('rules', () => {
  it('are made from the search they were opened with', async () => {
    vi.mocked(wireListRules).mockResolvedValue([])
    vi.mocked(wirePreviewRule).mockResolvedValue({ count: 4, items: [] })
    vi.mocked(wireCreateRule).mockResolvedValue(SHOP)
    renderAt('/settings?tab=rules&rule_query=from%3Ashop.example')
    expect((screen.getByPlaceholderText(/from:news/) as HTMLInputElement).value).toBe(
      'from:shop.example'
    )
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Shop' } })
    fireEvent.click(screen.getByLabelText('Archive'))
    fireEvent.change(screen.getByRole('combobox'), { target: { value: '3' } })
    fireEvent.click(screen.getByText('Preview'))
    expect(await screen.findByText('4 conversations match now')).toBeDefined()
    fireEvent.click(screen.getByText('Save rule'))
    await vi.waitFor(() =>
      expect(wireCreateRule).toHaveBeenCalledWith({
        actions: ['archive', 'label:3'],
        enabled: true,
        name: 'Shop',
        query: 'from:shop.example',
      })
    )
    expect(wireApplyRule).not.toHaveBeenCalled()
  })

  it('run over existing mail only after the count is confirmed', async () => {
    vi.mocked(wireListRules).mockResolvedValue([SHOP])
    vi.mocked(wirePreviewRule).mockResolvedValue({ count: 12, items: [] })
    vi.mocked(wireApplyRule).mockResolvedValue({ changed: 12, failed: 0, run_id: '7' })
    renderAt('/settings?tab=rules')
    expect(await screen.findByText('Archive, Label Shopping')).toBeDefined()
    fireEvent.click(screen.getByText('Apply to existing mail'))
    expect(await screen.findByText(/the 12 conversations that match now/)).toBeDefined()
    expect(wireApplyRule).not.toHaveBeenCalled()
    fireEvent.click(screen.getByText('Confirm'))
    await vi.waitFor(() => expect(wireApplyRule).toHaveBeenCalledWith('1'))
  })
})
