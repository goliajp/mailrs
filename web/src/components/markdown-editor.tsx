import { useCallback, useRef, useState } from 'react'
import Markdown from 'react-markdown'
import rehypeHighlight from 'rehype-highlight'
import remarkGfm from 'remark-gfm'

type FormatAction = {
  label: string
  icon: string
  prefix: string
  suffix: string
  block?: boolean
}

const ACTIONS: FormatAction[] = [
  { label: 'Bold', icon: 'B', prefix: '**', suffix: '**' },
  { label: 'Italic', icon: 'I', prefix: '_', suffix: '_' },
  { label: 'Code', icon: '</>', prefix: '`', suffix: '`' },
  { label: 'Code block', icon: '{ }', prefix: '```\n', suffix: '\n```', block: true },
  { label: 'Link', icon: 'Link', prefix: '[', suffix: '](url)' },
]

export function MarkdownEditor({
  value,
  onChange,
  onSubmit,
  placeholder,
  disabled,
  minRows,
}: {
  value: string
  onChange: (v: string) => void
  onSubmit: () => void
  placeholder?: string
  disabled?: boolean
  minRows?: number
}) {
  const ref = useRef<HTMLTextAreaElement>(null)
  const [dragOver, setDragOver] = useState(false)
  const [previewing, setPreviewing] = useState(false)

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
        e.preventDefault()
        onSubmit()
      }
    },
    [onSubmit]
  )

  const applyFormat = (action: FormatAction) => {
    const el = ref.current
    if (!el) return

    const start = el.selectionStart
    const end = el.selectionEnd
    const selected = value.slice(start, end)
    const replacement = `${action.prefix}${selected || action.label}${action.suffix}`

    const newValue = value.slice(0, start) + replacement + value.slice(end)
    onChange(newValue)

    // restore cursor position
    requestAnimationFrame(() => {
      const selectStart = start + action.prefix.length
      const selectEnd = selectStart + (selected || action.label).length
      el.focus()
      el.setSelectionRange(selectStart, selectEnd)
    })
  }

  const autoResize = (e: React.FormEvent<HTMLTextAreaElement>) => {
    const el = e.currentTarget
    el.style.height = 'auto'
    el.style.height = Math.min(el.scrollHeight, 200) + 'px'
  }

  const togglePreview = () => {
    setPreviewing((prev) => !prev)
  }

  return (
    <div
      className={`rounded-xl border ${
        dragOver
          ? 'border-[var(--color-brand-primary)] bg-[var(--color-brand-subtle)]'
          : 'border-[var(--color-border-default)] bg-[var(--color-bg-sunken)]'
      }`}
      onDragOver={(e) => {
        e.preventDefault()
        setDragOver(true)
      }}
      onDragLeave={() => setDragOver(false)}
    >
      {/* toolbar */}
      <div className="flex items-center gap-0.5 border-b border-[var(--color-border-default)] px-2 py-1">
        {!previewing &&
          ACTIONS.map((a) => (
            <button
              key={a.label}
              onClick={() => applyFormat(a)}
              className="rounded-md px-1.5 py-0.5 text-xs text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text-secondary)]"
              title={a.label}
              type="button"
            >
              {a.icon === 'B' ? (
                <span className="font-bold">{a.icon}</span>
              ) : a.icon === 'I' ? (
                <span className="italic">{a.icon}</span>
              ) : a.icon === '</>' ? (
                <span className="font-mono text-xs">{a.icon}</span>
              ) : a.icon === '{ }' ? (
                <span className="font-mono text-xs">{a.icon}</span>
              ) : (
                <span className="text-xs">{a.icon}</span>
              )}
            </button>
          ))}
        <div className="ml-auto flex items-center gap-1">
          <button
            type="button"
            onClick={togglePreview}
            className={`rounded-md px-1.5 py-0.5 text-xs font-medium transition-colors ${
              previewing
                ? 'bg-[var(--color-border-default)] text-[var(--color-text-secondary)]'
                : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-hover)] hover:text-[var(--color-text-secondary)]'
            }`}
            title={previewing ? 'Back to editing' : 'Preview Markdown'}
          >
            {previewing ? 'Edit' : 'Preview'}
          </button>
        </div>
      </div>

      {/* preview pane */}
      {previewing ? (
        <div
          className="prose prose-sm max-w-none px-4 py-2 prose-[var(--color-text-primary)]"
          style={{ minHeight: `${(minRows ?? 1) * 1.5}rem` }}
        >
          {value.trim() ? (
            <Markdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
              {value}
            </Markdown>
          ) : (
            <p className="text-[var(--color-text-tertiary)]">{placeholder ?? 'Nothing to preview'}</p>
          )}
        </div>
      ) : (
        /* edit textarea */
        <textarea
          ref={ref}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={handleKeyDown}
          onInput={autoResize}
          placeholder={placeholder}
          disabled={disabled}
          rows={minRows ?? 1}
          className="max-h-[200px] w-full resize-none bg-transparent px-4 py-2 text-sm text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)]"
          style={{ minHeight: `${(minRows ?? 1) * 1.5}rem` }}
        />
      )}
    </div>
  )
}
