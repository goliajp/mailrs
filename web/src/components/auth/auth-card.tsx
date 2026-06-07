import type { FormEvent, ReactNode } from 'react'

type AuthCardProps = {
  children: ReactNode
  onSubmit?: (e: FormEvent) => void
}

export function AuthCard({ children, onSubmit }: AuthCardProps) {
  if (onSubmit) {
    return (
      <div className="bg-bg flex min-h-screen items-center justify-center px-4">
        <form
          className="border-border bg-surface w-full max-w-sm space-y-5 rounded-lg border p-6 shadow-lg select-none sm:p-8"
          onSubmit={onSubmit}
        >
          {children}
        </form>
      </div>
    )
  }
  return (
    <div className="bg-bg flex min-h-screen items-center justify-center px-4">
      <div className="border-border bg-surface w-full max-w-sm space-y-5 rounded-lg border p-6 shadow-lg select-none sm:p-8">
        {children}
      </div>
    </div>
  )
}
