import type { FormatKind } from '@/lib/markdown-format'

import { Bold, Code, Italic, Link, List, ListOrdered, Quote } from 'lucide-react'

import { type Key, useT } from '@/i18n'

const BUTTONS: { icon: typeof Bold; kind: FormatKind; label: Key }[] = [
  { icon: Bold, kind: 'bold', label: 'composer.format.bold' },
  { icon: Italic, kind: 'italic', label: 'composer.format.italic' },
  { icon: Link, kind: 'link', label: 'composer.format.link' },
  { icon: List, kind: 'list', label: 'composer.format.list' },
  { icon: ListOrdered, kind: 'numbered', label: 'composer.format.numbered' },
  { icon: Quote, kind: 'quote', label: 'composer.format.quote' },
  { icon: Code, kind: 'code', label: 'composer.format.code' },
]

/** Markdown formatting for one text block. The buttons never take focus, so the selection survives. */
export function FormatToolbar({
  disabled,
  onFormat,
}: {
  disabled?: boolean
  onFormat: (kind: FormatKind) => void
}) {
  const t = useT()
  return (
    <div
      aria-label={t('composer.format.toolbar')}
      className="flex items-center gap-0.5"
      role="toolbar"
    >
      {BUTTONS.map(({ icon: Icon, kind, label }) => (
        <button
          aria-label={t(label)}
          className="text-fg-muted hover:bg-bg-secondary hover:text-fg flex h-6 w-6 items-center justify-center rounded transition-colors disabled:opacity-50"
          disabled={disabled}
          key={kind}
          onClick={() => onFormat(kind)}
          onMouseDown={(e) => e.preventDefault()}
          title={t(label)}
          type="button"
        >
          <Icon className="h-3.5 w-3.5" />
        </button>
      ))}
    </div>
  )
}
