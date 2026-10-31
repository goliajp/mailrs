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
import { type Key, useT } from '@/i18n'
import { formatFullDate } from '@/lib/format'
import { queryClient } from '@/lib/query-client'
import { settingsKeys } from '@/lib/query-keys'
import { wireIssueResetCode, wireResetCodeStatus } from '@/wire/endpoints/auth'
import { WireErrorException } from '@/wire/errors'

type Translate = (key: Key, vars?: Record<string, number | string>) => string

/**
 * Handing out reset codes: for the accounts whose password an
 * administrator cleared. A code is shown once, here, and is to be given to
 * the account's owner in person.
 */
export function ResetCodesSection() {
  const t = useT()
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
      <SectionHeader title={t('settings.cat.reset-codes')} />
      <p className="text-fg-secondary text-sm">{t('settings.resetCodes.help')}</p>

      <form className="flex gap-2" onSubmit={lookUp}>
        <input
          aria-label={t('settings.resetCodes.account')}
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
          {t('settings.resetCodes.lookUp')}
        </button>
      </form>

      {address !== '' && (
        <div className={cardClass}>
          <p className="mb-2 text-sm font-medium break-all">{address}</p>
          {status.error && <p className="text-danger text-sm">{refusal(status.error, t)}</p>}
          {status.data && <StatusLine status={status.data} />}
          <div className="mt-3">
            <button
              className={btnPrimary}
              disabled={issue.isPending}
              onClick={() => issue.mutate(address)}
              type="button"
            >
              {t('settings.resetCodes.issue')}
            </button>
          </div>
        </div>
      )}

      {issue.error && (
        <Alert role="alert" variant="danger">
          {refusal(issue.error, t)}
        </Alert>
      )}
      {issue.data && (
        <Alert role="status" variant="success">
          <p className="text-sm">{t('settings.resetCodes.give', { address })}</p>
          <p
            className="my-2 font-mono text-2xl tracking-widest select-all"
            data-testid="reset-code"
          >
            {issue.data.code.slice(0, 5)}-{issue.data.code.slice(5)}
          </p>
          <p className="text-sm">
            {t('settings.resetCodes.validUntil', { at: formatFullDate(issue.data.expires_at) })}
          </p>
        </Alert>
      )}
    </div>
  )
}

function issuedText(who: string, at: null | number, t: Translate): string {
  if (at === null) return t('settings.resetCodes.issuedBy', { who })
  return t('settings.resetCodes.issuedByAt', { at: formatFullDate(at), who })
}

/** Why an issue was refused, in words. */
function refusal(e: unknown, t: Translate): string {
  if (!(e instanceof WireErrorException)) return t('compose.networkError')
  const d = e.detail
  if (d.kind === 'not-found') return t('settings.resetCodes.noAccount')
  if (d.kind === 'forbidden') return t('settings.resetCodes.forbidden')
  if (d.kind === 'server' && d.message === 'password_not_cleared') {
    return t('settings.resetCodes.notCleared')
  }
  if (d.kind === 'server' && d.message === 'reset_codes_not_configured') {
    return t('settings.resetCodes.notConfigured')
  }
  return e.message
}

function StatusLine({ status }: { status: WireResetCodeStatus }) {
  const t = useT()
  if (!status.issued)
    return <p className="text-fg-muted text-sm">{t('settings.resetCodes.noneOut')}</p>
  return (
    <ul className="text-fg-secondary space-y-1 text-sm">
      <li>{issuedText(status.issued_by, status.issued_at, t)}</li>
      {status.expires_at !== null && (
        <li>{t('settings.resetCodes.validUntil', { at: formatFullDate(status.expires_at) })}</li>
      )}
      <li>{t('settings.resetCodes.wrongTries', { count: status.failures })}</li>
      {!status.key_current && <li className="text-danger">{t('settings.resetCodes.staleKey')}</li>}
    </ul>
  )
}
