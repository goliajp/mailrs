/**
 * Route-level error fallback — v2.1 §13.4 (2026-07-08).
 *
 * Rendered by react-router when any route (loader, action, or
 * component render) throws. Consumes `useRouteError()`; distinguishes
 * `WireErrorException` for wire-specific messages from arbitrary
 * runtime errors.
 */

import { Button } from '@goliapkg/gds'
import { useNavigate, useRouteError } from 'react-router'

import { WireErrorException } from '@/wire/errors'

export function RouteErrorFallback() {
  const err = useRouteError()
  const navigate = useNavigate()

  const message = describe(err)
  const isAuth = err instanceof WireErrorException && err.detail.kind === 'auth'

  return (
    <div className="flex min-h-[60vh] flex-col items-center justify-center gap-4 px-6 text-center">
      <h1 className="text-fg text-2xl font-semibold">Something went wrong</h1>
      <p className="text-fg-secondary max-w-md text-sm">{message}</p>
      <div className="flex gap-2">
        {isAuth ? (
          <Button onClick={() => navigate('/login')} variant="primary">
            Sign in
          </Button>
        ) : (
          <>
            <Button onClick={() => location.reload()} variant="primary">
              Reload
            </Button>
            <Button onClick={() => navigate('/')} variant="secondary">
              Home
            </Button>
          </>
        )}
      </div>
    </div>
  )
}

function describe(err: unknown): string {
  if (err instanceof WireErrorException) {
    switch (err.detail.kind) {
      case 'aborted':
        return 'Request was cancelled.'
      case 'auth':
        return 'Your session expired. Please sign in again.'
      case 'forbidden':
        return "You don't have permission to see this."
      case 'network':
        return "Couldn't reach the server. Check your connection."
      case 'not-found':
        return "We couldn't find what you were looking for."
      case 'server':
        return err.detail.message ?? 'The server hit an error.'
      case 'validation':
        return 'The server response was in an unexpected shape.'
      default:
        return 'An unexpected error occurred.'
    }
  }
  if (err instanceof Error) return err.message || 'An unexpected error occurred.'
  return 'An unexpected error occurred.'
}
