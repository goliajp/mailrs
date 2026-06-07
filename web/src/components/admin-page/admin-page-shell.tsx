import { useId } from 'react'

type AdminPageShellProps = {
  actions?: React.ReactNode
  children: React.ReactNode
  title: string
}

export function AdminPageShell({ actions, children, title }: AdminPageShellProps) {
  const titleId = useId()
  return (
    <main aria-labelledby={titleId} className="flex-1 overflow-y-auto p-6" role="region">
      <div className="mb-6 flex items-center justify-between gap-3">
        <h2 className="text-lg font-semibold" id={titleId}>
          {title}
        </h2>
        {actions && <div className="flex items-center gap-2">{actions}</div>}
      </div>
      {children}
    </main>
  )
}
