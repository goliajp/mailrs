import { useEffect } from 'react'

type Props = {
  open: boolean
  onClose: () => void
  title?: string
  children: React.ReactNode
}

export function Dialog({ open, onClose, title, children }: Props) {
  useEffect(() => {
    if (!open) return
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    document.addEventListener('keydown', handler)
    return () => document.removeEventListener('keydown', handler)
  }, [open, onClose])

  if (!open) return null

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        className="mx-4 w-full max-w-md border border-[var(--color-border-default)] bg-[var(--color-bg-overlay)] p-6 shadow-[var(--shadow-lg)]"
        onClick={(e) => e.stopPropagation()}
      >
        {title && (
          <h3 className="mb-3 text-sm font-semibold text-[var(--color-text-primary)]">{title}</h3>
        )}
        {children}
      </div>
    </div>
  )
}
