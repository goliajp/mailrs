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
      { keys: ['/'], description: 'Focus search' },
      { keys: ['?'], description: 'Show shortcuts' },
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
        className="w-full max-w-sm rounded-xl border border-zinc-200 bg-white p-6 shadow-2xl dark:border-zinc-700 dark:bg-zinc-900"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-5 flex items-center justify-between">
          <h2 className="text-base font-semibold text-zinc-900 dark:text-zinc-100">
            Keyboard Shortcuts
          </h2>
          <button
            onClick={onClose}
            className="flex h-7 w-7 items-center justify-center rounded-md text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-700 dark:hover:bg-zinc-800 dark:hover:text-zinc-300"
            aria-label="Close"
          >
            <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        <div className="space-y-5">
          {SHORTCUT_GROUPS.map((group) => (
            <div key={group.title}>
              <p className="mb-2 text-xs font-medium uppercase tracking-wider text-zinc-400 dark:text-zinc-500">
                {group.title}
              </p>
              <ul className="space-y-1.5">
                {group.shortcuts.map((shortcut) => (
                  <li
                    key={shortcut.description}
                    className="flex items-center justify-between gap-4"
                  >
                    <span className="text-sm text-zinc-600 dark:text-zinc-400">
                      {shortcut.description}
                    </span>
                    <span className="flex shrink-0 gap-1">
                      {shortcut.keys.map((key) => (
                        <kbd
                          key={key}
                          className="inline-flex h-6 min-w-[1.5rem] items-center justify-center rounded border border-zinc-300 bg-zinc-100 px-1.5 font-mono text-xs text-zinc-700 dark:border-zinc-600 dark:bg-zinc-800 dark:text-zinc-300"
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

        <p className="mt-5 text-center text-xs text-zinc-400 dark:text-zinc-600">
          Press <kbd className="inline-flex h-5 min-w-[1.25rem] items-center justify-center rounded border border-zinc-300 bg-zinc-100 px-1 font-mono text-xs text-zinc-600 dark:border-zinc-600 dark:bg-zinc-800 dark:text-zinc-400">?</kbd> or <kbd className="inline-flex h-5 min-w-[1.25rem] items-center justify-center rounded border border-zinc-300 bg-zinc-100 px-1 font-mono text-xs text-zinc-600 dark:border-zinc-600 dark:bg-zinc-800 dark:text-zinc-400">Esc</kbd> to close
        </p>
      </div>
    </div>
  )
}
