import type { WireResetCodeStatus } from '@/wire/schemas/auth'

import { Alert } from '@goliapkg/gds'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import {
  btnPrimary,
  btnSecondary,
  cardClass,
  inputClass,
  SectionHeader,
} from '@/components/settings/_shared'
import { formatFullDate } from '@/lib/format'
import { queryClient } from '@/lib/query-client'
import { settingsKeys } from '@/lib/query-keys'
import { wireIssueResetCode, wireResetCodeStatus } from '@/wire/endpoints/auth'
import { WireErrorException } from '@/wire/errors'

/**
 * Handing out reset codes: for the accounts whose password an
 * administrator cleared. A code is shown once, here, and is to be given to
 * the account's owner in person.
 */
export function ResetCodesSection() {
  const [draft, setDraft] = useState('')
  const [address, setAddress] = useState('')
  const status = useQuery({
    enabled: address !== '',
    queryKey: settingsKeys.resetCode(address),
    queryFn: () => wireResetCodeStatus(address),
  })
  const issue = useMutation({
    mutationFn: wireIssueResetCode,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: settingsKeys.resetCode(address) }),
  })

  const lookUp = (e: React.FormEvent) => {
    e.preventDefault()
    issue.reset()
    setAddress(draft.trim().toLowerCase())
  }

  return (
    <div className="space-y-4">
      <SectionHeader title="Reset codes" />
      <p className="text-fg-secondary text-sm">
        An account whose password was cleared gets back in with a code you hand its owner in person.
        A new code replaces the one before; it works once, for 24 hours.
      </p>

      <form className="flex gap-2" onSubmit={lookUp}>
        <input
          aria-label="Account"
          className={inputClass}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="someone@golia.jp"
          type="email"
          value={draft}
        />
        <button
          className={`${btnSecondary} shrink-0 whitespace-nowrap`}
          disabled={!draft.trim()}
          type="submit"
        >
          Look up
        </button>
      </form>

      {address !== '' && (
        <div className={cardClass}>
          <p className="mb-2 text-sm font-medium break-all">{address}</p>
          {status.error && <p className="text-danger text-sm">{refusal(status.error)}</p>}
          {status.data && <StatusLine status={status.data} />}
          <div className="mt-3">
            <button
              className={btnPrimary}
              disabled={issue.isPending}
              onClick={() => issue.mutate(address)}
              type="button"
            >
              Issue a code
            </button>
          </div>
        </div>
      )}

      {issue.error && (
        <Alert role="alert" variant="danger">
          {refusal(issue.error)}
        </Alert>
      )}
      {issue.data && (
        <Alert role="status" variant="success">
          <p className="text-sm">
            Give this code to the owner of {address} in person. It is shown only now.
          </p>
          <p
            className="my-2 font-mono text-2xl tracking-widest select-all"
            data-testid="reset-code"
          >
            {issue.data.code.slice(0, 5)}-{issue.data.code.slice(5)}
          </p>
          <p className="text-sm">Valid until {formatFullDate(issue.data.expires_at)}</p>
        </Alert>
      )}
    </div>
  )
}

/** Why an issue was refused, in words. */
function refusal(e: unknown): string {
  if (!(e instanceof WireErrorException)) return 'Network error'
  const d = e.detail
  if (d.kind === 'not-found') return 'There is no account with that address'
  if (d.kind === 'forbidden') return 'You are not one of the people who may hand out reset codes'
  if (d.kind === 'server' && d.message === 'password_not_cleared') {
    return 'That account still has its password: only an account whose password was cleared gets a code'
  }
  if (d.kind === 'server' && d.message === 'reset_codes_not_configured') {
    return 'Reset codes are not set up on this server (MAILRS_RESET_CODE_KEY)'
  }
  return e.message
}

function StatusLine({ status }: { status: WireResetCodeStatus }) {
  if (!status.issued)
    return <p className="text-fg-muted text-sm">No code is out for this account.</p>
  return (
    <ul className="text-fg-secondary space-y-1 text-sm">
      <li>
        Issued by {status.issued_by}
        {status.issued_at !== null && <>, {formatFullDate(status.issued_at)}</>}
      </li>
      {status.expires_at !== null && <li>Valid until {formatFullDate(status.expires_at)}</li>}
      <li>Wrong tries: {status.failures} of 5</li>
      {!status.key_current && (
        <li className="text-danger">
          Made with a key since replaced: it no longer works. Issue a new one.
        </li>
      )}
    </ul>
  )
}
