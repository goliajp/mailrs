import React from 'react'

type Props = {
  children: React.ReactNode
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
  }

  handleReload = () => {
    this.setState({ error: undefined, hasError: false })
    window.location.reload()
  }

  render() {
    if (this.state.hasError) {
      return (
        <div className="bg-bg-secondary flex min-h-screen items-center justify-center">
          <div className="border-border bg-surface w-full max-w-md rounded-lg border px-6 py-8 shadow-lg">
            <div className="text-center">
              <div className="mb-4 flex justify-center">
                <div className="bg-danger/10 rounded-md p-3">
                  <svg
                    className="text-danger h-8 w-8"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path
                      d="M12 8v4m0 4v.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                    />
                  </svg>
                </div>
              </div>
              <h1 className="text-fg mb-2 text-2xl font-bold">出错了</h1>
              <p className="text-fg-secondary mb-4">
                应用程序遇到了一个意外错误。请尝试重新加载页面。
              </p>
              {this.state.error && (
                <details className="mb-6 text-left">
                  <summary className="text-fg-secondary hover:text-fg cursor-pointer text-sm font-medium">
                    查看错误详情
                  </summary>
                  <pre className="bg-bg-secondary text-fg-secondary mt-2 overflow-auto rounded-md p-3 text-xs">
                    {this.state.error.toString()}
                  </pre>
                </details>
              )}
              <button
                className="bg-accent hover:bg-accent-hover w-full rounded-md px-4 py-2 font-semibold text-white transition-colors"
                onClick={this.handleReload}
              >
                重新加载
              </button>
            </div>
          </div>
        </div>
      )
    }

    return this.props.children
  }
}
