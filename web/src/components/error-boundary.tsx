import React from 'react'

interface Props {
  children: React.ReactNode
}

interface State {
  hasError: boolean
  error?: Error
}

export class ErrorBoundary extends React.Component<Props, State> {
  constructor(props: Props) {
    super(props)
    this.state = { hasError: false }
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error }
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    console.error('ErrorBoundary caught an error:', error, errorInfo)
  }

  handleReload = () => {
    this.setState({ hasError: false, error: undefined })
    window.location.reload()
  }

  render() {
    if (this.state.hasError) {
      return (
        <div className="flex items-center justify-center min-h-screen bg-gradient-to-br from-slate-50 to-slate-100">
          <div className="w-full max-w-md px-6 py-8 bg-white rounded shadow-lg">
            <div className="text-center">
              <div className="mb-4 flex justify-center">
                <div className="rounded bg-red-100 p-3">
                  <svg
                    className="h-8 w-8 text-red-600"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                      d="M12 8v4m0 4v.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
                    />
                  </svg>
                </div>
              </div>
              <h1 className="mb-2 text-2xl font-bold text-slate-900">
                出错了
              </h1>
              <p className="mb-4 text-slate-600">
                应用程序遇到了一个意外错误。请尝试重新加载页面。
              </p>
              {this.state.error && (
                <details className="mb-6 text-left">
                  <summary className="cursor-pointer text-sm font-medium text-slate-700 hover:text-slate-900">
                    查看错误详情
                  </summary>
                  <pre className="mt-2 overflow-auto rounded bg-slate-50 p-3 text-xs text-slate-700">
                    {this.state.error.toString()}
                  </pre>
                </details>
              )}
              <button
                onClick={this.handleReload}
                className="w-full rounded bg-blue-600 px-4 py-2 font-semibold text-white transition-colors hover:bg-blue-700 active:bg-blue-800"
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
