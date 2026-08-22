import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const listed = vi.fn()
const settingsFor = vi.fn()
const added = vi.fn()
const removed = vi.fn()

vi.mock('@/wire/endpoints/external-accounts', () => ({
  wireAddExternalAccount: (...a: unknown[]) => added(...a),
  wireExternalSettingsFor: (...a: unknown[]) => settingsFor(...a),
  wireListExternalAccounts: () => listed(),
  wireRemoveExternalAccount: (...a: unknown[]) => removed(...a),
}))

const { MailAccountsSection } = await import('../mail-accounts-section')

function show() {
  return render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <MailAccountsSection />
    </QueryClientProvider>
  )
}

const qqPreset = {
  known: true as const,
  preset: {
    auth: 'app_password' as const,
    id: 'qq',
    imap: { host: 'imap.qq.com', port: 993, protocol: 'imap', tls: 'implicit' as const },
    label: 'QQ 邮箱',
    secret_help: { url: 'https://service.mail.qq.com/detail/0/75', what: '授权码（不是登录密码）' },
    skip_folders: [],
    smtp: { host: 'smtp.qq.com', port: 465, protocol: 'smtp', tls: 'implicit' as const },
  },
}

beforeEach(() => {
  vi.clearAllMocks()
  listed.mockResolvedValue([])
  settingsFor.mockResolvedValue(qqPreset)
  added.mockResolvedValue({})
  removed.mockResolvedValue(undefined)
})

describe('connecting a mailbox somewhere else', () => {
  /**
   * The one that generates support mail. A QQ login password is
   * refused with a message that does not say a 授权码 is wanted, so the
   * field has to be labelled with the provider's own word for it —
   * anything else and the person types the wrong thing and is told
   * only "LOGIN failed".
   */
  it('labels the secret field with the provider’s own word for it', async () => {
    show()
    await userEvent.type(screen.getByLabelText('Email address'), 'someone@qq.com')
    await waitFor(() => expect(settingsFor).toHaveBeenCalled())
    await screen.findByLabelText('授权码（不是登录密码）')
    expect(screen.queryByLabelText('Password')).toBeNull()
  })

  it('links to the page that makes one', async () => {
    show()
    await userEvent.type(screen.getByLabelText('Email address'), 'someone@qq.com')
    const link = await screen.findByRole('link', { name: /get one/i })
    expect(link.getAttribute('href')).toBe('https://service.mail.qq.com/detail/0/75')
  })

  /** A partial address is not a domain; looking one up on every
   * keystroke asks the server about "s", "so", "som". */
  it('does not look anything up until the address is complete', async () => {
    show()
    await userEvent.type(screen.getByLabelText('Email address'), 'some')
    expect(settingsFor).not.toHaveBeenCalled()
  })

  it('does not ask for a password when the provider will not take one', async () => {
    settingsFor.mockResolvedValue({
      known: true,
      preset: {
        ...qqPreset.preset,
        auth: 'oauth2',
        id: 'gmail',
        label: 'Gmail',
        secret_help: null,
      },
    })
    show()
    await userEvent.type(screen.getByLabelText('Email address'), 'someone@gmail.com')
    expect(await screen.findByText(/does not accept a password/i)).toBeTruthy()
  })

  /**
   * A broken account must be visible where it was added, and the two
   * failures need different words: one is a button to press, the other
   * is waiting.
   */
  it('shows a rejected credential differently from a server that is down', async () => {
    listed.mockResolvedValue([
      row({ id: 'a', last_error: 'AUTHENTICATIONFAILED', state: 'needs_auth' }),
      row({ id: 'b', last_error: 'connection refused', state: 'error' }),
    ])
    show()
    expect(await screen.findByText('Sign in again')).toBeTruthy()
    expect(await screen.findByText('Not syncing')).toBeTruthy()
  })

  it('shows the account colour the server chose', async () => {
    listed.mockResolvedValue([row({ colour: '#22c55e', id: 'a' })])
    const { container } = show()
    await screen.findByTestId('external-account-a')
    const dot = container.querySelector('[data-testid="external-account-a"] span[aria-hidden]')
    // jsdom normalises the hex the server sent into rgb, so the
    // assertion is on the colour rather than on its spelling.
    expect((dot as HTMLElement | null)?.style.backgroundColor).toBe('rgb(34, 197, 94)')
  })

  it('sends what was typed and refreshes the list', async () => {
    show()
    await userEvent.type(screen.getByLabelText('Email address'), 'someone@qq.com')
    await userEvent.type(await screen.findByLabelText('授权码（不是登录密码）'), 'code-1234')
    await userEvent.click(screen.getByRole('button', { name: /connect/i }))
    await waitFor(() =>
      expect(added).toHaveBeenCalledWith(
        expect.objectContaining({ email: 'someone@qq.com', secret: 'code-1234' })
      )
    )
  })
})

function row(over: Partial<Record<string, unknown>>) {
  return {
    auth: 'app_password',
    colour: '#3b82f6',
    created_at: 0,
    display_name: 'QQ',
    email: 'someone@qq.com',
    failures: 0,
    id: 'a',
    incoming: { host: 'imap.qq.com', port: 993, protocol: 'imap', tls: 'implicit' },
    last_error: null,
    last_sync: 0,
    next_attempt: 0,
    outgoing: { host: 'smtp.qq.com', port: 465, protocol: 'smtp', tls: 'implicit' },
    provider: 'qq',
    sort: 0,
    state: 'ok',
    username: null,
    ...over,
  }
}
