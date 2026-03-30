import { X } from 'lucide-react'
import { useEffect } from 'react'

type ShortcutEntry = {
  description: string
  keys: string[]
}

type ShortcutGroup = {
  shortcuts: ShortcutEntry[]
  title: string
}

const SHORTCUT_GROUPS: ShortcutGroup[] = [
  {
    shortcuts: [
      { description: 'Next conversation', keys: ['j', '↓'] },
      { description: 'Previous conversation', keys: ['k', '↑'] },
      { description: 'Open conversation', keys: ['Enter'] },
      { description: 'Back to list', keys: ['Esc'] },
    ],
    title: 'Navigation',
  },
  {
    shortcuts: [
      { description: 'New conversation', keys: ['n'] },
      { description: 'Reply', keys: ['r'] },
      { description: 'Archive / Unarchive', keys: ['e'] },
      { description: 'Star / Unstar', keys: ['s'] },
      { description: 'Pin / Unpin', keys: ['p'] },
      { description: 'Mark unread', keys: ['u'] },
      { description: 'Mark read + next', keys: ['Shift+I'] },
      { description: 'Forward', keys: ['f'] },
      { description: 'Delete', keys: ['#'] },
      { description: 'Focus search', keys: ['/'] },
      { description: 'Show shortcuts', keys: ['?'] },
    ],
    title: 'Actions',
  },
  {
    shortcuts: [
      { description: 'Go to Inbox', keys: ['g', 'i'] },
      { description: 'Go to Sent', keys: ['g', 's'] },
      { description: 'Go to Action', keys: ['g', 'a'] },
    ],
    title: 'Go to',
  },
]

type Props = {
  onClose: () => void
  open: boolean
}

export function KeyboardShortcutsDialog({ onClose, open }: Props) {
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
      aria-label="Keyboard shortcuts"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm"
      onClick={onClose}
      role="dialog"
    >
      {/* panel — stop propagation so clicks inside don't close */}
      <div
        className="border-border bg-surface w-full max-w-sm rounded-lg border p-6 shadow-lg select-none"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-5 flex items-center justify-between">
          <h2 className="text-fg text-base font-semibold">
            Keyboard Shortcuts
          </h2>
          <button
            aria-label="Close"
            className="text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary flex h-7 w-7 items-center justify-center rounded-md transition-colors"
            onClick={onClose}
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="space-y-5">
          {SHORTCUT_GROUPS.map((group) => (
            <div key={group.title}>
              <p className="text-fg-muted mb-2 text-xs font-medium tracking-wider uppercase">
                {group.title}
              </p>
              <ul className="space-y-1.5">
                {group.shortcuts.map((shortcut) => (
                  <li
                    className="flex items-center justify-between gap-4"
                    key={shortcut.description}
                  >
                    <span className="text-fg-secondary text-sm">
                      {shortcut.description}
                    </span>
                    <span className="flex shrink-0 gap-1">
                      {shortcut.keys.map((key) => (
                        <kbd
                          className="border-border bg-surface text-fg-secondary inline-flex h-6 min-w-[1.5rem] items-center justify-center rounded border px-1.5 font-mono text-xs"
                          key={key}
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

        <p className="text-fg-muted mt-5 text-center text-xs">
          Press{' '}
          <kbd className="border-border bg-surface text-fg-secondary inline-flex h-5 min-w-[1.25rem] items-center justify-center rounded border px-1 font-mono text-xs">
            ?
          </kbd>{' '}
          or{' '}
          <kbd className="border-border bg-surface text-fg-secondary inline-flex h-5 min-w-[1.25rem] items-center justify-center rounded border px-1 font-mono text-xs">
            Esc
          </kbd>{' '}
          to close
        </p>
      </div>
    </div>
  )
}
