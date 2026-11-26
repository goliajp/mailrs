import type { WireResetCodeStatus } from '@/wire/schemas/auth'

import { Alert } from '@goliapkg/gds'
import { useMutation, useQuery } from '@tanstack/react-query'

import { btnPrimary } from '@/components/settings/_shared'
import { type Key, useT } from '@/i18n'
import { formatFullDate } from '@/lib/format'
import { queryClient } from '@/lib/query-client'
import { settingsKeys } from '@/lib/query-keys'
import { wireIssueResetCode, wireResetCodeStatus } from '@/wire/endpoints/auth'
import { WireErrorException } from '@/wire/errors'

type Translate = (key: Key, vars?: Record<string, number | string>) => string

/**
 * The reset code of one account: whether one is out, and a button that
 * issues a new one. A code is shown once, here, and is to be given to the
 * account's owner in person.
 */
export function ResetCodePanel({ address }: { address: string }) {
  const t = useT()
  const status = useQuery({
    queryKey: settingsKeys.resetCode(address),
    queryFn: () => wireResetCodeStatus(address),
  })
  const issue = useMutation({
    mutationFn: wireIssueResetCode,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: settingsKeys.resetCode(address) }),
  })

  return (
    <div className="space-y-3">
      {status.error && <p className="text-danger text-sm">{refusal(status.error, t)}</p>}
      {status.data && <StatusLine status={status.data} />}
      <button
        className={btnPrimary}
        disabled={issue.isPending}
        onClick={() => issue.mutate(address)}
        type="button"
      >
        {t('settings.resetCodes.issue')}
      </button>

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
