import type { TextBlockData } from '../types'

import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'

import { uploadInlineImage } from '@/components/rich-editor'

// the markdown-preview pipeline (react-markdown + remark-* + rehype-highlight)
// is ~150-200 kB. preview is opt-in via the "Preview" tab — most send paths
// only ever see the edit textarea — so lazy-load it to keep the chat hot
// path lean.
const MarkdownPreview = lazy(() => import('@/components/composer/blocks/markdown-preview'))

type Props = {
  data: TextBlockData
  disabled?: boolean
  onChange: (data: TextBlockData) => void
  onSubmit: () => void
  placeholder?: string
}

const PREVIEW_PROSE_CLASS =
  'prose prose-sm prose-fg max-w-none px-3 py-2 ' +
  'prose-pre:bg-[#1e1e2e] prose-pre:text-[#cdd6f4] prose-pre:rounded-md ' +
  'prose-code:before:content-none prose-code:after:content-none ' +
  'prose-p:my-0 prose-headings:my-2 prose-p:leading-6'

const MIN_HEIGHT_PX = 240

export function TextBlock({ data, disabled, onChange, onSubmit, placeholder }: Props) {
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const [tab, setTab] = useState<'edit' | 'preview'>('edit')

  const onSubmitRef = useRef(onSubmit)
  useEffect(() => {
    onSubmitRef.current = onSubmit
  }, [onSubmit])

  // imperative resize: textarea grows to fit content, never shrinks below min
  const resize = useCallback(() => {
    const el = textareaRef.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${Math.max(el.scrollHeight, MIN_HEIGHT_PX)}px`
  }, [])

  useLayoutEffect(() => {
    resize()
  }, [data.content, resize, tab])

  const emitChange = useCallback(() => {
    const el = textareaRef.current
    if (!el) return
    onChange({ content: el.value, format: 'markdown', html: '' })
  }, [onChange])

  const handleChange = useCallback(
    (e: React.ChangeEvent<HTMLTextAreaElement>) => {
      const content = e.target.value
      onChange({ content, format: 'markdown', html: '' })
    },
    [onChange]
  )

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
        e.preventDefault()
        onSubmitRef.current()
        return
      }

      // tab key → insert two spaces, do not move focus
      if (e.key === 'Tab' && !e.shiftKey) {
        e.preventDefault()
        insertAtCursor(textareaRef.current, '  ')
        emitChange()
        return
      }

      // enter → preserve list-marker prefix on the next line
      if (e.key === 'Enter' && !e.shiftKey) {
        const el = textareaRef.current
        if (!el) return
        const { selectionStart, value } = el
        const lineStart = value.lastIndexOf('\n', selectionStart - 1) + 1
        const line = value.slice(lineStart, selectionStart)
        const match = /^(\s*)([-*]|\d+\.)\s+/.exec(line)
        if (!match) return
        const [, indent, marker] = match
        // empty marker line → strip it on Enter
        if (line === match[0]) {
          e.preventDefault()
          replaceRange(el, lineStart, selectionStart, '')
          emitChange()
          return
        }
        e.preventDefault()
        const nextMarker = /^\d+\.$/.test(marker) ? `${parseInt(marker, 10) + 1}.` : marker
        insertAtCursor(el, `\n${indent}${nextMarker} `)
        emitChange()
      }
    },
    [emitChange]
  )

  // image paste → upload, insert markdown image syntax at caret
  const handlePaste = useCallback(
    async (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
      const items = Array.from(e.clipboardData.items).filter((i) => i.type.startsWith('image/'))
      if (items.length === 0) return
      e.preventDefault()
      const el = textareaRef.current
      if (!el) return
      for (const item of items) {
        const file = item.getAsFile()
        if (!file) continue
        const url = await uploadInlineImage(file)
        if (!url) continue
        insertAtCursor(el, `![${file.name || 'image'}](${url})`)
      }
      emitChange()
    },
    [emitChange]
  )

  // image drop directly on the textarea → markdown syntax (other files bubble
  // up to the StructuredCompose drop zone for attachment handling)
  const handleDrop = useCallback(
    async (e: React.DragEvent<HTMLTextAreaElement>) => {
      const files = Array.from(e.dataTransfer.files).filter((f) => f.type.startsWith('image/'))
      if (files.length === 0) return
      e.preventDefault()
      e.stopPropagation()
      const el = textareaRef.current
      if (!el) return
      for (const file of files) {
        const url = await uploadInlineImage(file)
        if (!url) continue
        insertAtCursor(el, `![${file.name || 'image'}](${url})`)
      }
      emitChange()
    },
    [emitChange]
  )

  return (
    <div className="flex flex-col">
      <div className="border-border flex shrink-0 items-center gap-1 border-b px-3 py-1.5">
        <TabButton active={tab === 'edit'} onClick={() => setTab('edit')}>
          Write
        </TabButton>
        <TabButton active={tab === 'preview'} onClick={() => setTab('preview')}>
          Preview
        </TabButton>
        <span className="text-fg-muted ml-auto text-[11px]">Markdown · Cmd+Enter to send</span>
      </div>

      {tab === 'edit' ? (
        <textarea
          className="text-fg placeholder:text-fg-muted block w-full resize-none border-0 bg-transparent px-3 py-2 text-sm leading-6 outline-none"
          disabled={disabled}
          onChange={handleChange}
          onDrop={handleDrop}
          onInput={resize}
          onKeyDown={handleKeyDown}
          onPaste={handlePaste}
          placeholder={placeholder ?? 'Write your message in markdown…'}
          ref={textareaRef}
          spellCheck
          style={{ minHeight: `${MIN_HEIGHT_PX}px` }}
          value={data.content}
        />
      ) : (
        <PreviewBody content={data.content} />
      )}
    </div>
  )
}

function insertAtCursor(el: HTMLTextAreaElement | null, text: string) {
  if (!el) return
  const start = el.selectionStart
  const end = el.selectionEnd
  const before = el.value.slice(0, start)
  const after = el.value.slice(end)
  el.value = before + text + after
  const caret = start + text.length
  el.selectionStart = el.selectionEnd = caret
}

// match the textarea's vertical rhythm: each blank line in the source becomes
// a &nbsp; paragraph in the rendered output. N consecutive newlines means
// (N-1) blank lines between paragraphs, so we emit (N-1) &nbsp; paragraphs
// flanked by the surrounding paragraph break.
function preserveBlankLines(md: string): string {
  return md.replace(/\n{2,}/g, (match) => '\n\n' + '&nbsp;\n\n'.repeat(match.length - 1))
}

function PreviewBody({ content }: { content: string }) {
  const processed = useMemo(() => preserveBlankLines(content), [content])
  if (!content.trim()) {
    return (
      <div className={`${PREVIEW_PROSE_CLASS} min-h-[240px]`}>
        <p className="text-fg-muted text-sm">Nothing to preview yet.</p>
      </div>
    )
  }
  return (
    <div className={`${PREVIEW_PROSE_CLASS} min-h-[240px]`}>
      <Suspense fallback={<p className="text-fg-muted text-sm">Loading preview…</p>}>
        <MarkdownPreview content={processed} />
      </Suspense>
    </div>
  )
}

function replaceRange(el: HTMLTextAreaElement | null, start: number, end: number, text: string) {
  if (!el) return
  const before = el.value.slice(0, start)
  const after = el.value.slice(end)
  el.value = before + text + after
  const caret = start + text.length
  el.selectionStart = el.selectionEnd = caret
}

function TabButton({
  active,
  children,
  onClick,
}: {
  active: boolean
  children: React.ReactNode
  onClick: () => void
}) {
  return (
    <button
      className={
        active
          ? 'bg-bg-secondary text-fg rounded-md px-2 py-0.5 text-xs font-medium'
          : 'text-fg-muted hover:bg-bg-secondary hover:text-fg rounded-md px-2 py-0.5 text-xs'
      }
      onClick={onClick}
      type="button"
    >
      {children}
    </button>
  )
}
