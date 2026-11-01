import type { ReplyMode } from './types'

import { Pencil } from 'lucide-react'

import { type Key, useT } from '@/i18n'

import { MODE_LABELS } from './types'

type ModeToggleProps = {
  fieldsOpen: boolean
  mode: ReplyMode
  onChange: (mode: ReplyMode) => void
  onToggleFields: () => void
  /** Whom the reply goes to now, as typed or as it started. */
  recipients: string
}

export function ModeToggle({
  fieldsOpen,
  mode,
  onChange,
  onToggleFields,
  recipients,
}: ModeToggleProps) {
  const t = useT()
  return (
    <div className="border-border flex shrink-0 items-center gap-1 border-b px-4 py-2 select-none">
      {(Object.keys(MODE_LABELS) as ReplyMode[]).map((m) => (
        <button
          aria-pressed={mode === m}
          className={modeClass(mode === m)}
          key={m}
          onClick={() => onChange(m)}
          type="button"
        >
          {t(MODE_LABELS[m])}
        </button>
      ))}
      {mode !== 'forward' && (
        <>
          <span className="text-fg-muted ml-auto min-w-0 truncate text-xs" title={recipients}>
            {t('compose.toRecipients', { recipients })}
          </span>
          <button
            aria-expanded={fieldsOpen}
            aria-label={t(toggleLabel(fieldsOpen))}
            className="text-fg-muted hover:bg-bg-secondary hover:text-fg flex h-7 w-7 shrink-0 items-center justify-center rounded-md transition-colors"
            onClick={onToggleFields}
            title={t(toggleLabel(fieldsOpen))}
            type="button"
          >
            <Pencil className="h-3.5 w-3.5" />
          </button>
        </>
      )}
    </div>
  )
}

function modeClass(active: boolean): string {
  const base =
    'focus-visible:ring-accent cursor-pointer shrink-0 rounded px-2.5 py-2 text-xs font-medium whitespace-nowrap transition-colors focus-visible:ring-2 focus-visible:outline-none md:py-1'
  if (active) return `${base} bg-accent/10 text-accent`
  return `${base} text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary`
}

function toggleLabel(open: boolean): Key {
  if (open) return 'composer.hideFields'
  return 'composer.editFields'
}
