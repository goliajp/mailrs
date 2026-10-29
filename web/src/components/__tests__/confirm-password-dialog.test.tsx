/**
 * The dialog behind every `confirm_required`: it tells the waiting request
 * whether to try again, asks for the one-time code only when the server
 * does, and says how many tries are left before the session ends.
 */

import type { WireConfirmOutcome } from '@/wire/endpoints/auth'

import { act, fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

const outcomes: WireConfirmOutcome[] = []
const sent: Array<{ code?: string; password: string }> = []
vi.mock('@/wire/endpoints/auth', () => ({
  wireConfirm: async (password: string, code?: string) => {
    sent.push({ code, password })
    return outcomes.shift()
  },
  wireLogout: async () => {},
}))

const { ConfirmPasswordHost } = await import('../confirm-password-dialog')
const { askToConfirm } = await import('@/lib/confirm-asker')

function mount() {
  render(
    <MemoryRouter>
      <ConfirmPasswordHost />
    </MemoryRouter>
  )
}

async function submit(password: string) {
  fireEvent.change(screen.getByLabelText('Password'), { target: { value: password } })
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }))
  })
}

describe('ConfirmPasswordHost', () => {
  afterEach(() => {
    outcomes.length = 0
    sent.length = 0
  })

  it('resolves true once the server accepts, after a wrong try and a code', async () => {
    mount()
    let answer: boolean | undefined
    act(() => {
      void askToConfirm().then((a) => (answer = a))
    })
    expect(await screen.findByText('Confirm it’s you')).toBeTruthy()

    outcomes.push({ kind: 'wrong', remaining: 4 })
    await submit('nope')
    expect(screen.getByRole('alert').textContent).toBe(
      'Wrong password or code. 4 tries left before you are signed out'
    )
    expect(screen.queryByLabelText('Authenticator code')).toBeNull()

    outcomes.push({ kind: 'code-needed' })
    await submit('right')
    expect(screen.getByLabelText('Authenticator code')).toBeTruthy()

    outcomes.push({ kind: 'confirmed' })
    fireEvent.change(screen.getByLabelText('Authenticator code'), { target: { value: '123456' } })
    await submit('right')
    expect(sent.at(-1)).toEqual({ code: '123456', password: 'right' })
    expect(answer).toBe(true)
    expect(screen.queryByText('Confirm it’s you')).toBeNull()
  })

  it('resolves false when cancelled', async () => {
    mount()
    let answer: boolean | undefined
    act(() => {
      void askToConfirm().then((a) => (answer = a))
    })
    await screen.findByText('Confirm it’s you')
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    })
    expect(answer).toBe(false)
    expect(sent).toEqual([])
  })
})
