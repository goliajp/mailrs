import { Alert, Button } from '@goliapkg/gds'

type AdminErrorStateProps = {
  error: Error | string | unknown
  onRetry?: () => void
  retryDisabled?: boolean
  /** what failed to load, e.g. "the domain list"; the title names it */
  what?: string
}

/** Says what could not be loaded and the server's own reason, with a retry. */
export function AdminErrorState({ error, onRetry, retryDisabled, what }: AdminErrorStateProps) {
  return (
    <Alert role="alert" title={titleOf(what)} variant="danger">
      <div className="space-y-2">
        <p className="text-sm">{messageOf(error)}</p>
        {onRetry && (
          <Button disabled={retryDisabled} onClick={onRetry} size="sm" variant="secondary">
            Try again
          </Button>
        )}
      </div>
    </Alert>
  )
}

function messageOf(error: unknown): string {
  if (error instanceof Error) return error.message
  if (typeof error === 'string') return error
  return 'The server did not say why'
}

function titleOf(what: string | undefined): string {
  if (what === undefined) return 'Could not load this page'
  return `Could not load ${what}`
}
