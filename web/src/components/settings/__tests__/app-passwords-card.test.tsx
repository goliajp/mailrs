// App passwords in settings: made with a device name and shown once in
// groups of four, listed without the password, revoked one at a time; and,
// where the password is changed, all revoked at once.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/wire/endpoints/app-passwords', () => ({
  wireCreateAppPassword: vi.fn(),
  wireListAppPasswords: vi.fn(),
  wireRevokeAllAppPasswords: vi.fn(),
  wireRevokeAppPassword: vi.fn(),
}))
vi.mock('@/lib/query-client', () => ({ queryClient: { invalidateQueries: vi.fn() } }))

import { AppPasswordsCard, RevokeAllAppPasswords } from '@/components/settings/app-passwords-card'
import { grouped } from '@/lib/app-passwords'
import {
  wireCreateAppPassword,
  wireListAppPasswords,
  wireRevokeAllAppPasswords,
  wireRevokeAppPassword,
} from '@/wire/endpoints/app-passwords'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

const PHONE = { created_at: 1, id: 'k1', last_used_at: null, name: 'iPhone Mail' }

function wrap(node: React.ReactNode) {
  return render(<QueryClientProvider client={new QueryClient()}>{node}</QueryClientProvider>)
}

describe('app passwords', () => {
  it('groups a password in fours', () => {
    expect(grouped('abcdefghijkmnpqr')).toBe('abcd efgh ijkm npqr')
  })

  it('makes one, shows it once, and revokes', async () => {
    vi.mocked(wireListAppPasswords).mockResolvedValue([PHONE])
    vi.mocked(wireCreateAppPassword).mockResolvedValue({
      id: 'k2',
      name: 'Laptop',
      password: 'abcdefghijkmnpqr',
    })
    vi.mocked(wireRevokeAppPassword).mockResolvedValue()
    wrap(<AppPasswordsCard />)
    expect(await screen.findByText('iPhone Mail')).toBeDefined()
    expect(screen.getByText(/never used/)).toBeDefined()
    const create = screen.getByText('Create') as HTMLButtonElement
    expect(create.disabled).toBe(true)
    fireEvent.change(screen.getByLabelText('Device name'), { target: { value: ' Laptop ' } })
    fireEvent.click(create)
    await vi.waitFor(() => expect(wireCreateAppPassword).toHaveBeenCalledWith('Laptop'))
    expect(await screen.findByText('abcd efgh ijkm npqr')).toBeDefined()
    fireEvent.click(screen.getByText('Done'))
    expect(screen.queryByText('abcd efgh ijkm npqr')).toBeNull()
    fireEvent.click(screen.getByText('Revoke'))
    await vi.waitFor(() => expect(wireRevokeAppPassword).toHaveBeenCalledWith('k1'))
  })

  it('offers to revoke all where the password is changed, only when there are some', async () => {
    vi.mocked(wireListAppPasswords).mockResolvedValue([PHONE])
    vi.mocked(wireRevokeAllAppPasswords).mockResolvedValue({ revoked: 1 })
    wrap(<RevokeAllAppPasswords />)
    fireEvent.click(await screen.findByText('Revoke all app passwords'))
    await vi.waitFor(() => expect(wireRevokeAllAppPasswords).toHaveBeenCalled())
    cleanup()
    vi.mocked(wireListAppPasswords).mockResolvedValue([])
    const { container } = wrap(<RevokeAllAppPasswords />)
    await vi.waitFor(() => expect(wireListAppPasswords).toHaveBeenCalled())
    expect(container.textContent).toBe('')
  })
})
