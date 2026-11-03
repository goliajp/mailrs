import type { TextBlockData } from '../types'

import { useCallback, useEffect, useLayoutEffect, useRef } from 'react'

import { uploadInlineImage } from '@/components/composer/editor-extensions'
import { FormatToolbar } from '@/components/composer/format-toolbar'
import { useT } from '@/i18n'
import { applyFormat, formatForKey, type FormatKind } from '@/lib/markdown-format'

// path lean.

type Props = {
  data: TextBlockData
  disabled?: boolean
  onChange: (data: TextBlockData) => void
  onSubmit: () => void
  placeholder?: string
}

const MIN_HEIGHT_PX = 240

export function TextBlock({ data, disabled, onChange, onSubmit, placeholder }: Props) {
  const t = useT()
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  const onSubmitRef = useRef(onSubmit)
  useEffect(() => {
    onSubmitRef.current = onSubmit
  }, [onSubmit])

  // imperative resize: textarea grows to fit content, never shrinks below min
  const resize = useCallback(() => {
    const el = textareaRef.current
    if (!el) return
    // An empty box is the minimum height, and asking for `scrollHeight`
    // lays out the whole page there and then. The reply box mounts empty
    // with every conversation opened, inside the commit that draws it.
    if (el.value === '') {
      el.style.height = `${MIN_HEIGHT_PX}px`
      return
    }
    el.style.height = 'auto'
    el.style.height = `${Math.max(el.scrollHeight, MIN_HEIGHT_PX)}px`
  }, [])

  // the selection a format asked for, put back once the new value is rendered
  const pendingSelection = useRef<null | { end: number; start: number }>(null)
  useLayoutEffect(() => {
    resize()
    const el = textareaRef.current
    const sel = pendingSelection.current
    if (!el || !sel) return
    pendingSelection.current = null
    el.setSelectionRange(sel.start, sel.end)
  }, [data.content, resize])

  const format = useCallback(
    (kind: FormatKind) => {
      const el = textareaRef.current
      if (!el) return
      const edit = applyFormat(el.value, el.selectionStart, el.selectionEnd, kind)
      pendingSelection.current = { end: edit.end, start: edit.start }
      onChange({ content: edit.value, format: 'markdown', html: '' })
      el.focus()
    },
    [onChange]
  )

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
      const kind = formatForKey(e.key)
      if ((e.ctrlKey || e.metaKey) && kind) {
        e.preventDefault()
        format(kind)
        return
      }
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
    [emitChange, format]
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
        <FormatToolbar disabled={disabled} onFormat={format} />
        <span className="text-fg-muted text-mini ml-auto min-w-0 truncate whitespace-nowrap">
          {t('composer.markdownHint')}
        </span>
      </div>

      <textarea
        className="text-fg placeholder:text-fg-muted block w-full resize-none border-0 bg-transparent px-3 py-2 text-sm leading-6 outline-none"
        disabled={disabled}
        onChange={handleChange}
        onDrop={handleDrop}
        onInput={resize}
        onKeyDown={handleKeyDown}
        onPaste={handlePaste}
        placeholder={placeholder ?? t('composer.markdownPlaceholder')}
        ref={textareaRef}
        spellCheck
        style={{ minHeight: `${MIN_HEIGHT_PX}px` }}
        value={data.content}
      />
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

function replaceRange(el: HTMLTextAreaElement | null, start: number, end: number, text: string) {
  if (!el) return
  const before = el.value.slice(0, start)
  const after = el.value.slice(end)
  el.value = before + text + after
  const caret = start + text.length
  el.selectionStart = el.selectionEnd = caret
}
