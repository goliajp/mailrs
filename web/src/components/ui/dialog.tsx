import { useEffect } from 'react'

type Props = {
  children: React.ReactNode
  onClose: () => void
  open: boolean
  title?: string
}

export function Dialog({ children, onClose, open, title }: Props) {
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
        aria-modal="true"
        className="mx-4 w-full max-w-md border border-[var(--color-border-default)] bg-[var(--color-bg-overlay)] p-6 shadow-[var(--shadow-lg)]"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
      >
        {title && (
          <h3 className="mb-3 text-sm font-semibold text-[var(--color-text-primary)]">
            {title}
          </h3>
        )}
        {children}
      </div>
    </div>
  )
}
