import { AlertTriangle } from 'lucide-react'
import React from 'react'

import { reportRuntimeError } from '@/lib/error-report'

type ErrorBoundaryLevel = 'app' | 'route'

type Props = {
  children: React.ReactNode
  level?: ErrorBoundaryLevel
}

type State = {
  error?: Error
  hasError: boolean
}

export class ErrorBoundary extends React.Component<Props, State> {
  constructor(props: Props) {
    super(props)
    this.state = { hasError: false }
  }

  static getDerivedStateFromError(error: Error): State {
    return { error, hasError: true }
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    console.error('ErrorBoundary caught an error:', error, errorInfo)
    reportRuntimeError({ error })
  }

  handleAppReload = () => {
    this.setState({ error: undefined, hasError: false })
    window.location.reload()
  }

  handleRouteReset = () => {
    this.setState({ error: undefined, hasError: false })
  }

  render() {
    if (!this.state.hasError) {
      return this.props.children
    }

    const level = this.props.level ?? 'app'
    return level === 'route' ? (
      <RouteErrorPanel error={this.state.error} onRetry={this.handleRouteReset} />
    ) : (
      <AppErrorScreen error={this.state.error} onReload={this.handleAppReload} />
    )
  }
}

function AppErrorScreen({ error, onReload }: { error?: Error; onReload: () => void }) {
  return (
    <div
      className="bg-bg-secondary flex min-h-[100dvh] items-center justify-center px-4"
      role="alert"
    >
      <div className="border-border bg-surface w-full max-w-md rounded-lg border px-6 py-8 shadow-lg">
        <div className="text-center">
          <div className="mb-4 flex justify-center">
            <div className="bg-danger/10 rounded-md p-3">
              <AlertTriangle aria-hidden="true" className="text-danger h-8 w-8" />
            </div>
          </div>
          <h1 className="text-fg mb-2 text-2xl font-bold">Something went wrong</h1>
          <p className="text-fg-secondary mb-4">
            The app hit an unexpected error. Try reloading the page.
          </p>
          {error && (
            <details className="mb-6 text-left">
              <summary className="text-fg-secondary hover:text-fg cursor-pointer text-sm font-medium">
                Error details
              </summary>
              <pre className="bg-bg-secondary text-fg-secondary mt-2 overflow-auto rounded-md p-3 text-xs">
                {error.toString()}
              </pre>
            </details>
          )}
          <button
            className="bg-accent hover:bg-accent-hover w-full rounded-md px-4 py-2 font-semibold text-white transition-colors"
            onClick={onReload}
            type="button"
          >
            Reload
          </button>
        </div>
      </div>
    </div>
  )
}

function RouteErrorPanel({ error, onRetry }: { error?: Error; onRetry: () => void }) {
  return (
    <div className="flex h-full min-h-0 flex-1 items-center justify-center p-6" role="alert">
      <div className="border-border bg-surface flex max-w-md flex-col items-center gap-3 rounded-lg border p-6 text-center shadow-sm">
        <div className="bg-danger/10 rounded-md p-2">
          <AlertTriangle aria-hidden="true" className="text-danger h-5 w-5" />
        </div>
        <p className="text-fg text-sm font-medium">This page failed to load</p>
        <p className="text-fg-muted text-xs">
          Other parts of the app should still work. Retry, or use the sidebar to navigate elsewhere.
        </p>
        {error && (
          <details className="w-full text-left">
            <summary className="text-fg-secondary hover:text-fg cursor-pointer text-xs font-medium">
              Error details
            </summary>
            <pre className="bg-bg-secondary text-fg-secondary mt-2 overflow-auto rounded-md p-2 text-[11px]">
              {error.toString()}
            </pre>
          </details>
        )}
        <button
          className="bg-accent hover:bg-accent-hover mt-1 rounded-md px-3 py-1.5 text-xs font-medium text-white transition-colors"
          onClick={onRetry}
          type="button"
        >
          Try again
        </button>
      </div>
    </div>
  )
}
