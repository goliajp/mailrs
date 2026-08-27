import { X } from 'lucide-react'

import { MobileModal } from '@/components/mobile-modal'
import { SHORTCUT_GROUPS } from '@/lib/shortcuts'

type Props = {
  onClose: () => void
  open: boolean
}

export function KeyboardShortcutsDialog({ onClose, open }: Props) {
  if (!open) return null

  return (
    <MobileModal onClose={onClose} open>
      {/* panel — stop propagation so clicks inside don't close */}
      <div
        className="border-border bg-surface w-full max-w-sm rounded-lg border p-6 shadow-lg select-none"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-5 flex items-center justify-between">
          <h2 className="text-fg text-base font-semibold">Keyboard Shortcuts</h2>
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
                    <span className="text-fg-secondary text-sm">{shortcut.description}</span>
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
    </MobileModal>
  )
}
