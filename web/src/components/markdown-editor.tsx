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
          ? 'border-blue-400 bg-blue-50 dark:bg-blue-950/20'
          : 'border-zinc-200 bg-zinc-50 dark:border-zinc-700 dark:bg-zinc-900'
      }`}
      onDragOver={(e) => {
        e.preventDefault()
        setDragOver(true)
      }}
      onDragLeave={() => setDragOver(false)}
    >
      {/* toolbar */}
      <div className="flex items-center gap-0.5 border-b border-zinc-200 px-2 py-1 dark:border-zinc-700">
        {!previewing &&
          ACTIONS.map((a) => (
            <button
              key={a.label}
              onClick={() => applyFormat(a)}
              className="rounded px-1.5 py-0.5 text-xs text-zinc-500 transition-colors hover:bg-zinc-200 hover:text-zinc-700 dark:hover:bg-zinc-700 dark:hover:text-zinc-300"
              title={a.label}
              type="button"
            >
              {a.icon === 'B' ? (
                <span className="font-bold">{a.icon}</span>
              ) : a.icon === 'I' ? (
                <span className="italic">{a.icon}</span>
              ) : a.icon === '</>' ? (
                <span className="font-mono text-[10px]">{a.icon}</span>
              ) : a.icon === '{ }' ? (
                <span className="font-mono text-[10px]">{a.icon}</span>
              ) : (
                <span className="text-[10px]">{a.icon}</span>
              )}
            </button>
          ))}
        <div className="ml-auto flex items-center gap-1">
          <button
            type="button"
            onClick={togglePreview}
            className={`rounded px-1.5 py-0.5 text-[10px] font-medium transition-colors ${
              previewing
                ? 'bg-zinc-200 text-zinc-700 dark:bg-zinc-700 dark:text-zinc-300'
                : 'text-zinc-400 hover:bg-zinc-200 hover:text-zinc-600 dark:hover:bg-zinc-700 dark:hover:text-zinc-400'
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
          className="prose prose-sm max-w-none px-4 py-2 dark:prose-invert"
          style={{ minHeight: `${(minRows ?? 1) * 1.5}rem` }}
        >
          {value.trim() ? (
            <Markdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
              {value}
            </Markdown>
          ) : (
            <p className="text-zinc-400">{placeholder ?? 'Nothing to preview'}</p>
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
          className="max-h-[200px] w-full resize-none bg-transparent px-4 py-2 text-sm text-zinc-900 outline-none placeholder:text-zinc-400 dark:text-zinc-100"
          style={{ minHeight: `${(minRows ?? 1) * 1.5}rem` }}
        />
      )}
    </div>
  )
}
