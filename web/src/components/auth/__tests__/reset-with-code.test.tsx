import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { contactText } from '@/lib/password-clear'

import { ResetWithCode } from '../reset-with-code'

const CONTACT = { address: 'lihao@golia.jp', name: 'Li Hao' }

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

function fill(code: string, password: string, repeat: string) {
  fireEvent.change(screen.getByLabelText('Reset code'), { target: { value: code } })
  fireEvent.change(screen.getByLabelText('New password'), { target: { value: password } })
  fireEvent.change(screen.getByLabelText('New password again'), { target: { value: repeat } })
  fireEvent.click(screen.getByRole('button', { name: 'Set new password' }))
}

describe('contactText', () => {
  it('names whom to ask, or an administrator', () => {
    expect(contactText(CONTACT)).toBe('Li Hao (lihao@golia.jp)')
    expect(contactText({ address: 'lihao@golia.jp', name: '' })).toBe('lihao@golia.jp')
    expect(contactText(null)).toBe('an administrator')
  })
})

describe('ResetWithCode', () => {
  it('says whom to ask for a code', () => {
    render(
      <ResetWithCode address="a@golia.jp" contact={CONTACT} onCancel={vi.fn()} onDone={vi.fn()} />
    )
    expect(screen.getByText(/Ask Li Hao \(lihao@golia.jp\) for a reset code/)).toBeTruthy()
  })

  it('stops two different passwords before sending anything', () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)
    render(
      <ResetWithCode address="a@golia.jp" contact={CONTACT} onCancel={vi.fn()} onDone={vi.fn()} />
    )
    fill('ABCDE23456', 'a sound new passphrase', 'a different passphrase')
    expect(screen.getByText('The two passwords are not the same')).toBeTruthy()
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('sends the code and the new password, and hands back the address', async () => {
    const fetchMock = vi.fn(async () => new Response(null, { status: 204 }))
    vi.stubGlobal('fetch', fetchMock)
    const onDone = vi.fn()
    render(
      <ResetWithCode address=" a@golia.jp " contact={CONTACT} onCancel={vi.fn()} onDone={onDone} />
    )
    fill('abcde-23456', 'a sound new passphrase', 'a sound new passphrase')
    await waitFor(() => expect(onDone).toHaveBeenCalledWith('a@golia.jp'))
    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(url).toContain('/api/auth/password-reset')
    expect(JSON.parse(String(init.body))).toEqual({
      address: 'a@golia.jp',
      code: 'abcde-23456',
      new_password: 'a sound new passphrase',
    })
  })

  it('tells a refused code apart and says whom to ask', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () => new Response(JSON.stringify({ error: 'invalid_reset_code' }), { status: 400 })
      )
    )
    const onDone = vi.fn()
    render(
      <ResetWithCode address="a@golia.jp" contact={CONTACT} onCancel={vi.fn()} onDone={onDone} />
    )
    fill('WRONG23456', 'a sound new passphrase', 'a sound new passphrase')
    await waitFor(() => expect(screen.getByText(/That code does not work/)).toBeTruthy())
    expect(screen.getByText(/Ask Li Hao \(lihao@golia.jp\) for a new code/)).toBeTruthy()
    expect(onDone).not.toHaveBeenCalled()
  })
})
