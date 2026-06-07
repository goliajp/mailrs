import { Alert, Button } from '@goliapkg/gds'

type AdminErrorStateProps = {
  error: Error | string | unknown
  onRetry?: () => void
  retryDisabled?: boolean
}

export function AdminErrorState({ error, onRetry, retryDisabled }: AdminErrorStateProps) {
  const message =
    error instanceof Error
      ? error.message
      : typeof error === 'string'
        ? error
        : 'Failed to load data'

  return (
    <Alert role="alert" title="Couldn't load this page" variant="danger">
      <div className="space-y-2">
        <p className="text-sm">{message}</p>
        {onRetry && (
          <Button disabled={retryDisabled} onClick={onRetry} size="sm" variant="secondary">
            Try again
          </Button>
        )}
      </div>
    </Alert>
  )
}
