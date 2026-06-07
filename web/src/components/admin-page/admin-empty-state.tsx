import { Inbox } from 'lucide-react'

type AdminEmptyStateProps = {
  action?: React.ReactNode
  description?: string
  icon?: React.ReactNode
  title: string
}

export function AdminEmptyState({ action, description, icon, title }: AdminEmptyStateProps) {
  return (
    <div className="border-border bg-bg-secondary/50 flex flex-col items-center justify-center rounded-lg border border-dashed px-6 py-12 text-center">
      <div className="text-fg-muted mb-3">{icon ?? <Inbox className="h-10 w-10" />}</div>
      <p className="text-fg text-sm font-medium">{title}</p>
      {description && <p className="text-fg-muted mt-1 max-w-md text-xs">{description}</p>}
      {action && <div className="mt-4">{action}</div>}
    </div>
  )
}
