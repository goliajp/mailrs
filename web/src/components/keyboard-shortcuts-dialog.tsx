import { X } from 'lucide-react'
import { useEffect } from 'react'

type ShortcutEntry = {
  keys: string[]
  description: string
}

type ShortcutGroup = {
  title: string
  shortcuts: ShortcutEntry[]
}

const SHORTCUT_GROUPS: ShortcutGroup[] = [
  {
    title: 'Navigation',
    shortcuts: [
      { keys: ['j', '↓'], description: 'Next conversation' },
      { keys: ['k', '↑'], description: 'Previous conversation' },
      { keys: ['Enter'], description: 'Open conversation' },
      { keys: ['Esc'], description: 'Back to list' },
    ],
  },
  {
    title: 'Actions',
    shortcuts: [
      { keys: ['n'], description: 'New conversation' },
      { keys: ['r'], description: 'Reply' },
      { keys: ['e'], description: 'Archive / Unarchive' },
      { keys: ['s'], description: 'Star / Unstar' },
      { keys: ['p'], description: 'Pin / Unpin' },
      { keys: ['u'], description: 'Mark unread' },
      { keys: ['Shift+I'], description: 'Mark read + next' },
      { keys: ['f'], description: 'Forward' },
      { keys: ['#'], description: 'Delete' },
      { keys: ['/'], description: 'Focus search' },
      { keys: ['?'], description: 'Show shortcuts' },
    ],
  },
  {
    title: 'Go to',
    shortcuts: [
      { keys: ['g', 'i'], description: 'Go to Inbox' },
      { keys: ['g', 's'], description: 'Go to Sent' },
      { keys: ['g', 'a'], description: 'Go to Action' },
    ],
  },
]

type Props = {
  open: boolean
  onClose: () => void
}

export function KeyboardShortcutsDialog({ open, onClose }: Props) {
  useEffect(() => {
    if (!open) return

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' || e.key === '?') {
        e.preventDefault()
        onClose()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [open, onClose])

  if (!open) return null

  return (
    // overlay
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm"
      onClick={onClose}
      aria-modal="true"
      role="dialog"
      aria-label="Keyboard shortcuts"
    >
      {/* panel — stop propagation so clicks inside don't close */}
      <div
        className="w-full max-w-sm select-none rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] p-6 shadow-lg"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-5 flex items-center justify-between">
          <h2 className="text-base font-semibold text-[var(--color-text-primary)]">
            Keyboard Shortcuts
          </h2>
          <button
            onClick={onClose}
            className="flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text-secondary)]"
            aria-label="Close"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="space-y-5">
          {SHORTCUT_GROUPS.map((group) => (
            <div key={group.title}>
              <p className="mb-2 text-xs font-medium uppercase tracking-wider text-[var(--color-text-tertiary)]">
                {group.title}
              </p>
              <ul className="space-y-1.5">
                {group.shortcuts.map((shortcut) => (
                  <li
                    key={shortcut.description}
                    className="flex items-center justify-between gap-4"
                  >
                    <span className="text-sm text-[var(--color-text-secondary)]">
                      {shortcut.description}
                    </span>
                    <span className="flex shrink-0 gap-1">
                      {shortcut.keys.map((key) => (
                        <kbd
                          key={key}
                          className="inline-flex h-6 min-w-[1.5rem] items-center justify-center rounded border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] px-1.5 font-mono text-xs text-[var(--color-text-secondary)]"
                        >
                          {key}
                        </kbd>
                      ))}
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>

        <p className="mt-5 text-center text-xs text-[var(--color-text-tertiary)]">
          Press <kbd className="inline-flex h-5 min-w-[1.25rem] items-center justify-center rounded border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] px-1 font-mono text-xs text-[var(--color-text-secondary)]">?</kbd> or <kbd className="inline-flex h-5 min-w-[1.25rem] items-center justify-center rounded border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] px-1 font-mono text-xs text-[var(--color-text-secondary)]">Esc</kbd> to close
        </p>
      </div>
    </div>
  )
}
