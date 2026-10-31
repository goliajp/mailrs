import { Alert, Button } from '@goliapkg/gds'

import { type Key, useT } from '@/i18n'
import { isWireError } from '@/wire/errors'

type AdminErrorStateProps = {
  error: Error | string | unknown
  onRetry?: () => void
  retryDisabled?: boolean
  /** what failed to load, e.g. "the domain list"; the title names it */
  what?: string
}

/**
 * Says what could not be loaded and the server's own reason, with a retry.
 * A feature switched off on purpose is a notice, not a failure: it says
 * why and offers no retry, since retrying cannot change the answer.
 */
export function AdminErrorState({ error, onRetry, retryDisabled, what }: AdminErrorStateProps) {
  const t = useT()
  if (isWireError(error) && error.detail.kind === 'disabled') {
    return (
      <Alert role="status" title={t('admin.featureClosed')} variant="info">
        <p className="text-sm">{error.detail.message}</p>
      </Alert>
    )
  }
  return (
    <Alert role="alert" title={titleOf(what, t)} variant="danger">
      <div className="space-y-2">
        <p className="text-sm">{messageOf(error, t('admin.noReason'))}</p>
        {onRetry && (
          <Button disabled={retryDisabled} onClick={onRetry} size="sm" variant="secondary">
            {t('common.tryAgain')}
          </Button>
        )}
      </div>
    </Alert>
  )
}

function messageOf(error: unknown, noReason: string): string {
  if (error instanceof Error) return error.message
  if (typeof error === 'string') return error
  return noReason
}

function titleOf(
  what: string | undefined,
  t: (key: Key, vars?: Record<string, string>) => string
): string {
  if (what === undefined) return t('admin.loadPageFailed')
  return t('admin.loadFailed', { what })
}
