import { ChevronRight, Mail } from 'lucide-react'

export function Section({
  action,
  children,
  icon: Icon,
  title,
}: {
  action?: { label: string; onClick: () => void }
  children: React.ReactNode
  icon: typeof Mail
  title: string
}) {
  return (
    <div className="border-border overflow-hidden rounded-lg border">
      <div className="border-border flex items-center justify-between border-b px-4 py-2.5">
        <div className="flex items-center gap-2">
          <Icon aria-hidden="true" className="text-fg-muted h-4 w-4" />
          <h3 className="text-fg text-sm font-medium">{title}</h3>
        </div>
        {action && (
          <button
            className="text-accent hover:text-accent-hover flex items-center gap-1 text-xs transition-colors"
            onClick={action.onClick}
          >
            {action.label}
            <ChevronRight className="h-3 w-3" />
          </button>
        )}
      </div>
      <div className="p-2">{children}</div>
    </div>
  )
}
