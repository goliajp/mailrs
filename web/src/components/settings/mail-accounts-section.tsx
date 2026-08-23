import type { ManualEndpoint } from '@/lib/manual-endpoints'
import type { WireExternalAccount } from '@/wire/schemas/external-accounts'

import { useQuery } from '@tanstack/react-query'
import { useEffect, useState } from 'react'

import { emptyEndpoint, manualEndpoints } from '@/lib/manual-endpoints'
import { queryClient } from '@/lib/query-client'
import { settingsKeys } from '@/lib/query-keys'
import {
  wireAddExternalAccount,
  wireExternalSettingsFor,
  wireListExternalAccounts,
  wireRemoveExternalAccount,
  wireSetExternalAccountPaused,
} from '@/wire/endpoints/external-accounts'

import { btnPrimary, inputClass, SectionHeader } from './_shared'
import { ManualServerFields } from './manual-server-fields'

type PresetOf = Extract<
  Awaited<ReturnType<typeof wireExternalSettingsFor>>,
  { known: true }
>['preset']

/**
 * Mailboxes somewhere else.
 *
 * Adding one is meant to be an address and a password. The address is
 * enough to know where Gmail's servers are; what it cannot know is the
 * password, and for half the providers the thing to type is not the
 * password at all but a code generated in their web UI. So the form
 * asks for the address first, looks the provider up, and only then
 * shows a secret field — labelled with the provider's own word for it
 * and with a link to the page that makes one.
 *
 * Typing a login password into a field labelled "授权码" is a mistake
 * somebody can recover from. Typing it into one labelled "Password" and
 * being refused with `LOGIN failed` is not.
 */
export function MailAccountsSection() {
  const accountsQuery = useQuery({
    queryKey: settingsKeys.externalAccounts(),
    queryFn: () => wireListExternalAccounts(),
  })
  const accounts = accountsQuery.data ?? []

  const [email, setEmail] = useState('')
  const [secret, setSecret] = useState('')
  const [name, setName] = useState('')
  const [error, setError] = useState('')
  const [adding, setAdding] = useState(false)
  // Shut unless somebody opens it: autodiscovery covers the providers
  // people use, and a form that opens with eight empty boxes teaches
  // everybody that connecting mail is hard.
  const [manual, setManual] = useState(false)
  const [incoming, setIncoming] = useState<ManualEndpoint>(emptyEndpoint('imap'))
  const [outgoing, setOutgoing] = useState<ManualEndpoint>(emptyEndpoint('smtp'))
  const [username, setUsername] = useState('')

  // Looked up as the address is typed, but only once it is an address:
  // a lookup on every keystroke of "s", "so", "som" is noise.
  const complete = /^[^@\s]+@[^@\s.]+\.[^@\s]+$/.test(email.trim())
  const settingsQuery = useQuery({
    enabled: complete,
    queryKey: settingsKeys.externalSettings(email.trim().toLowerCase()),
    staleTime: 5 * 60 * 1000,
    queryFn: () => wireExternalSettingsFor(email.trim()),
  })
  const preset = settingsQuery.data?.known === true ? settingsQuery.data.preset : null

  useEffect(() => {
    if (accountsQuery.error) {
      setError(
        accountsQuery.error instanceof Error ? accountsQuery.error.message : 'failed to load'
      )
    }
  }, [accountsQuery.error])

  const refresh = () => queryClient.invalidateQueries({ queryKey: settingsKeys.externalAccounts() })

  const handleAdd = async () => {
    setError('')
    if (!complete) {
      setError('Enter the full email address of the account to add')
      return
    }
    if (!secret.trim()) {
      setError(secretLabel(preset?.secret_help?.what) + ' is required')
      return
    }
    setAdding(true)
    try {
      const endpoints = manual ? manualEndpoints(incoming, outgoing) : null
      if (manual && !endpoints) {
        setError('Both servers need a name and a port')
        setAdding(false)
        return
      }
      await wireAddExternalAccount({
        display_name: name.trim() || undefined,
        email: email.trim(),
        secret: secret.trim(),
        username: manual && username.trim() ? username.trim() : undefined,
        ...(endpoints ?? {}),
      })
      setEmail('')
      setSecret('')
      setName('')
      setManual(false)
      setIncoming(emptyEndpoint('imap'))
      setOutgoing(emptyEndpoint('smtp'))
      setUsername('')
      await refresh()
    } catch (e) {
      setError(e instanceof Error ? e.message : 'could not add the account')
    } finally {
      setAdding(false)
    }
  }

  const handlePause = async (a: WireExternalAccount) => {
    setError('')
    try {
      await wireSetExternalAccountPaused(a.id, a.state !== 'paused')
      await refresh()
    } catch (e) {
      setError(e instanceof Error ? e.message : 'could not change the account')
    }
  }

  const handleRemove = async (a: WireExternalAccount) => {
    setError('')
    try {
      await wireRemoveExternalAccount(a.id)
      await refresh()
    } catch (e) {
      setError(e instanceof Error ? e.message : 'could not remove the account')
    }
  }

  return (
    <div className="space-y-6">
      <div>
        <SectionHeader title="Mail accounts" />
        <p className="text-fg-muted -mt-3 text-sm">
          Read and send from Gmail, Outlook, QQ or any IMAP server, in the same lists as this one.
        </p>
      </div>

      {error && (
        <p className="text-danger text-sm" role="alert">
          {error}
        </p>
      )}

      <ul className="space-y-2" data-testid="external-account-list">
        {accounts.map((a) => (
          <AccountRow
            account={a}
            key={a.id}
            onPause={() => void handlePause(a)}
            onRemove={() => void handleRemove(a)}
          />
        ))}
        {accounts.length === 0 && !accountsQuery.isFetching && (
          <li className="text-fg-muted text-sm">No other accounts connected yet.</li>
        )}
      </ul>

      <div className="border-border space-y-3 border-t pt-4">
        <h3 className="text-fg text-sm font-medium">Connect an account</h3>
        <input
          aria-label="Email address"
          autoComplete="email"
          className={inputClass}
          onChange={(e) => setEmail(e.target.value)}
          placeholder="you@gmail.com"
          type="email"
          value={email}
        />

        {complete && settingsQuery.isFetching && (
          <p className="text-fg-muted text-xs">Looking up {email.split('@')[1]}…</p>
        )}

        {preset && <ProviderNote preset={preset} />}
        {settingsQuery.data?.known === false && (
          <p className="text-fg-muted text-xs">
            No preset for this domain — its server settings will be discovered from DNS when the
            account is added.
          </p>
        )}

        {complete && (
          <>
            <input
              aria-label={secretLabel(preset?.secret_help?.what)}
              autoComplete="off"
              className={inputClass}
              onChange={(e) => setSecret(e.target.value)}
              placeholder={secretLabel(preset?.secret_help?.what)}
              type="password"
              value={secret}
            />
            <input
              aria-label="Name for this account"
              className={inputClass}
              onChange={(e) => setName(e.target.value)}
              placeholder="Name it (optional) — Work, 大学"
              type="text"
              value={name}
            />
            <button
              className="text-fg-muted hover:text-fg text-xs underline"
              onClick={() => setManual(!manual)}
              type="button"
            >
              {manual ? 'Discover the servers for me' : 'Enter the server settings myself'}
            </button>
            {manual && (
              <ManualServerFields
                incoming={incoming}
                onIncoming={setIncoming}
                onOutgoing={setOutgoing}
                onUsername={setUsername}
                outgoing={outgoing}
                username={username}
              />
            )}
          </>
        )}

        <button
          className={btnPrimary}
          disabled={adding || !complete}
          onClick={() => void handleAdd()}
          type="button"
        >
          {adding ? 'Connecting…' : 'Connect'}
        </button>
      </div>
    </div>
  )
}

function AccountRow({
  account,
  onPause,
  onRemove,
}: {
  account: WireExternalAccount
  onPause: () => void
  onRemove: () => void
}) {
  return (
    <li
      className="border-border flex items-center gap-3 rounded-md border px-3 py-2"
      data-testid={`external-account-${account.id}`}
    >
      <span
        aria-hidden
        className="h-2.5 w-2.5 shrink-0 rounded-full"
        // The one place a colour is not a Tailwind class: it is data on
        // the row, chosen by the server so all three clients agree.
        style={{ backgroundColor: account.colour ?? '#6b7280' }}
      />
      <span className="min-w-0 flex-1">
        <span className="text-fg block truncate text-sm">{account.display_name}</span>
        <span className="text-fg-muted block truncate text-xs">{account.email}</span>
        {/* The reason, where somebody reads it. It was in a `title`,
            which is a hover tooltip — invisible on a phone, and this
            is the one line that says what to do next. */}
        {account.state !== 'ok' && account.last_error && (
          <span className="text-fg-muted mt-0.5 block text-xs break-words">
            {truncate(account.last_error)}
          </span>
        )}
        {/* Work in progress, not a fault: a re-read after the server
            renumbered a folder moves a mailbox's worth of mail, and
            silence for that long reads as a stall. */}
        {account.progress && (
          <span className="text-fg-muted mt-0.5 block text-xs break-words">
            {truncate(account.progress)}
          </span>
        )}
      </span>
      <AccountState account={account} />
      {/* Not offered for an account whose credential was refused:
          pausing cannot fix that, and resuming would put it back on a
          timer that cannot succeed. */}
      {account.state !== 'needs_auth' && (
        <button
          aria-label={`${account.state === 'paused' ? 'Resume' : 'Pause'} ${account.email}`}
          className="text-fg-muted hover:text-fg text-xs"
          onClick={onPause}
        >
          {account.state === 'paused' ? 'Resume' : 'Pause'}
        </button>
      )}
      <button
        aria-label={`Remove ${account.email}`}
        className="text-fg-muted hover:text-danger text-xs"
        onClick={onRemove}
        type="button"
      >
        Remove
      </button>
    </li>
  )
}

/**
 * A broken account has to be visible here.
 *
 * Silence means somebody believes they are seeing all their mail when
 * they are not — and the two failures need different words, because one
 * is a button to press and the other is waiting.
 */
function AccountState({ account }: { account: WireExternalAccount }) {
  if (account.state === 'needs_auth') {
    return (
      <span className="bg-warning/10 text-warning rounded px-1.5 py-0.5 text-xs">
        Sign in again
      </span>
    )
  }
  if (account.state === 'error') {
    return (
      <span
        className="bg-danger/10 text-danger rounded px-1.5 py-0.5 text-xs"
        title={account.last_error ?? ''}
      >
        Not syncing
      </span>
    )
  }
  if (account.state === 'paused') {
    return <span className="text-fg-muted text-xs">Paused</span>
  }
  return null
}

function ProviderNote({ preset }: { preset: NonNullable<PresetOf> }) {
  if (preset.auth === 'oauth2') {
    return (
      <div className="space-y-2">
        <p className="text-fg-muted text-xs">
          {preset.label} does not accept a password for mail apps — connecting it opens its own
          sign-in page.
        </p>
        {/* A link, not a fetch: the provider's consent screen has to be
            a real navigation, and coming back is a redirect it controls. */}
        <a
          className={btnPrimary}
          href={`/api/accounts/external/oauth/${preset.id === 'gmail' ? 'google' : 'microsoft'}`}
        >
          Sign in to {preset.label}
        </a>
      </div>
    )
  }
  const help = preset.secret_help
  if (!help) return <p className="text-fg-muted text-xs">{preset.label}</p>
  return (
    <p className="text-fg-muted text-xs">
      {preset.label} wants a <span className="text-fg">{help.what}</span>, not your login password.{' '}
      <a className="text-accent hover:underline" href={help.url} rel="noreferrer" target="_blank">
        Get one
      </a>
    </p>
  )
}

/** The provider's own word for what to type, or a plain one. */
function secretLabel(what: null | string | undefined): string {
  return what && what.length > 0 ? what : 'Password'
}

/**
 * A provider's refusal, cut to a row.
 *
 * Usually one sentence; IMAP servers have been known to answer with a
 * paragraph and a URL, and a row that grows to fill the screen is its
 * own problem.
 */
function truncate(v: string): string {
  return v.length > 200 ? `${v.slice(0, 200)}…` : v
}
