import { useCallback, useEffect, useImperativeHandle, useRef, useState, forwardRef } from 'react'
import { useEditor, EditorContent, type Editor } from '@tiptap/react'
import { ChevronDown, ChevronRight, Code2, Eye, Type } from 'lucide-react'
import { marked } from 'marked'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeHighlight from 'rehype-highlight'

import {
  EditorToolbar,
  createEditorExtensions,
  createMinimalExtensions,
  uploadInlineImage,
  PROSE_CLASS,
} from '@/components/rich-editor'

export type EditorMode = 'rich' | 'markdown' | 'preview'

export type StructuredContent = {
  compose: { text: string; html: string }
  signature: { text: string; html: string }
  quoted: { text: string; html: string }
  fullText: string
  fullHtml: string
}

export type StructuredComposeHandle = {
  getContent: () => StructuredContent
  getComposeEditor: () => Editor | null
  clearCompose: () => void
  setComposeContent: (html: string) => void
  getEditorMode: () => EditorMode
}

type Props = {
  onSubmit: () => void
  placeholder?: string
  disabled?: boolean
  signature?: string
  signatureEnabled?: boolean
  quotedHtml?: string
  quotedHeader?: string
  quotedHeaderHtml?: string
  mode?: 'new' | 'reply' | 'forward'
}

const SIG_SEPARATOR_TEXT = '\n\n-- \n'

type MdAction = { label: string; icon: string; prefix: string; suffix: string; block?: boolean }
const MD_ACTIONS: MdAction[] = [
  { label: 'Bold', icon: 'B', prefix: '**', suffix: '**' },
  { label: 'Italic', icon: 'I', prefix: '_', suffix: '_' },
  { label: 'Code', icon: '</>', prefix: '`', suffix: '`' },
  { label: 'Code block', icon: '{ }', prefix: '```\n', suffix: '\n```', block: true },
  { label: 'Link', icon: 'Link', prefix: '[', suffix: '](url)' },
  { label: 'Heading', icon: 'H', prefix: '## ', suffix: '', block: true },
  { label: 'List', icon: '•', prefix: '- ', suffix: '', block: true },
  { label: 'Quote', icon: '"', prefix: '> ', suffix: '', block: true },
]

function assembleContent(
  composeText: string,
  composeHtml: string,
  sig: Editor | null,
  quoted: Editor | null,
  quotedHeader: string,
): StructuredContent {
  const c = { text: composeText, html: composeHtml }
  const s = sig && sig.getText().trim()
    ? { text: sig.getText(), html: sig.getHTML() }
    : { text: '', html: '' }
  const q = quoted && quoted.getText().trim()
    ? { text: quoted.getText(), html: quoted.getHTML() }
    : { text: '', html: '' }

  let fullText = c.text
  if (s.text) fullText += SIG_SEPARATOR_TEXT + s.text
  if (q.text) fullText += '\n\n' + quotedHeader + q.text

  let fullHtml = c.html
  if (s.html) fullHtml += '<div class="email-signature" style="color:#888;margin-top:1em"><p>-- </p>' + s.html + '</div>'
  if (q.html) fullHtml += '<blockquote style="margin-top:1em;padding-left:0.75em;border-left:2px solid #ccc;color:#888">' + q.html + '</blockquote>'

  return { compose: c, signature: s, quoted: q, fullText, fullHtml }
}

function markdownToHtml(md: string): string {
  return marked.parse(md, { async: false, gfm: true, breaks: true }) as string
}

export const StructuredCompose = forwardRef<StructuredComposeHandle, Props>(function StructuredCompose(
  {
    onSubmit,
    placeholder,
    disabled,
    signature,
    signatureEnabled,
    quotedHtml,
    quotedHeader = '',
    quotedHeaderHtml,
    mode = 'new',
  },
  ref,
) {
  const [editorMode, setEditorMode] = useState<EditorMode>('rich')
  const [markdownText, setMarkdownText] = useState('')
  const [isDragOver, setIsDragOver] = useState(false)
  const [quotedExpanded, setQuotedExpanded] = useState(false)
  const dragCountRef = useRef(0)
  const sigInitializedRef = useRef(false)
  const quotedInitializedRef = useRef(false)
  const mdTextareaRef = useRef<HTMLTextAreaElement>(null)
  const scrollAreaRef = useRef<HTMLDivElement>(null)

  // compose editor: full capabilities
  const composeEditor = useEditor({
    extensions: createEditorExtensions(placeholder),
    editorProps: {
      attributes: {
        class: PROSE_CLASS + ' min-h-[3rem]',
        style: 'cursor: text',
      },
      handleKeyDown: (_view, event) => {
        if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') {
          event.preventDefault()
          onSubmit()
          return true
        }
        if (event.key === 'Tab' && composeEditor?.isActive('codeBlock')) {
          event.preventDefault()
          if (event.shiftKey) return true
          composeEditor?.commands.insertContent('  ')
          return true
        }
        return false
      },
    },
    editable: !disabled,
  })

  // signature editor
  const sigEditor = useEditor({
    extensions: createMinimalExtensions(),
    editorProps: {
      attributes: { class: 'prose prose-sm max-w-none px-3 py-1.5 outline-none text-[var(--color-text-tertiary)]' },
    },
    editable: !disabled,
  })

  // quoted editor: read-only
  const quotedEditor = useEditor({
    extensions: createMinimalExtensions(),
    editorProps: {
      attributes: { class: 'prose prose-sm max-w-none px-3 py-2 outline-none text-[var(--color-text-tertiary)]' },
    },
    editable: false,
  })

  // initialize signature
  useEffect(() => {
    if (!sigEditor) return
    if (signatureEnabled && signature?.trim()) {
      if (!sigInitializedRef.current) {
        sigEditor.commands.setContent(`<p>${signature.split('\n').map((l) => l || '<br>').join('</p><p>')}</p>`)
        sigInitializedRef.current = true
      }
    } else {
      sigEditor.commands.clearContent()
      sigInitializedRef.current = false
    }
  }, [sigEditor, signature, signatureEnabled])

  // initialize quoted content
  useEffect(() => {
    if (!quotedEditor || !quotedHtml) return
    if (!quotedInitializedRef.current) {
      const html = quotedHeaderHtml ? quotedHeaderHtml + quotedHtml : quotedHtml
      quotedEditor.commands.setContent(html)
      quotedInitializedRef.current = true
    }
  }, [quotedEditor, quotedHtml, quotedHeaderHtml])

  useEffect(() => {
    quotedInitializedRef.current = false
  }, [quotedHtml])

  // click anywhere in the empty space → focus editor
  const handleAreaClick = useCallback((e: React.MouseEvent<HTMLDivElement>) => {
    // only if clicking the scroll area itself, not a child element
    if (e.target !== scrollAreaRef.current) return
    if (editorMode === 'rich' && composeEditor) {
      composeEditor.commands.focus('end')
    } else if (editorMode === 'markdown' && mdTextareaRef.current) {
      mdTextareaRef.current.focus()
    }
  }, [editorMode, composeEditor])

  // mode switching
  const switchMode = useCallback((newMode: EditorMode) => {
    if (newMode === editorMode) return
    if (editorMode === 'rich' && newMode === 'markdown') {
      setMarkdownText(composeEditor?.getText() ?? '')
    } else if (editorMode === 'markdown' && newMode === 'rich') {
      const html = markdownToHtml(markdownText)
      composeEditor?.commands.setContent(html)
    }
    setEditorMode(newMode)
  }, [editorMode, composeEditor, markdownText])

  const getComposeContent = useCallback((): { text: string; html: string } => {
    if (editorMode === 'markdown') {
      return { text: markdownText, html: markdownToHtml(markdownText) }
    }
    return {
      text: composeEditor?.getText() ?? '',
      html: composeEditor?.getHTML() ?? '',
    }
  }, [editorMode, markdownText, composeEditor])

  useImperativeHandle(ref, () => ({
    getContent: () => {
      const { text, html } = getComposeContent()
      return assembleContent(text, html, sigEditor, quotedEditor, quotedHeader)
    },
    getComposeEditor: () => composeEditor,
    clearCompose: () => {
      composeEditor?.commands.clearContent()
      setMarkdownText('')
    },
    setComposeContent: (html: string) => {
      if (editorMode === 'markdown') {
        setMarkdownText(html.replace(/<[^>]*>/g, ''))
      } else {
        composeEditor?.commands.setContent(html)
      }
    },
    getEditorMode: () => editorMode,
  }), [composeEditor, sigEditor, quotedEditor, quotedHeader, editorMode, markdownText, getComposeContent])

  // drag-drop
  const handleDrop = useCallback(async (e: React.DragEvent) => {
    setIsDragOver(false)
    dragCountRef.current = 0
    if (!composeEditor || editorMode !== 'rich') return
    const files = Array.from(e.dataTransfer.files).filter((f) => f.type.startsWith('image/'))
    if (files.length === 0) return
    e.preventDefault()
    for (const file of files) {
      const url = await uploadInlineImage(file)
      if (url) composeEditor.chain().focus().setImage({ src: url }).run()
    }
  }, [composeEditor, editorMode])

  const handlePaste = useCallback(async (e: React.ClipboardEvent) => {
    if (!composeEditor || editorMode !== 'rich') return
    const items = Array.from(e.clipboardData.items).filter((i) => i.type.startsWith('image/'))
    if (items.length === 0) return
    e.preventDefault()
    for (const item of items) {
      const file = item.getAsFile()
      if (!file) continue
      const url = await uploadInlineImage(file)
      if (url) composeEditor.chain().focus().setImage({ src: url }).run()
    }
  }, [composeEditor, editorMode])

  const handleDragEnter = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    dragCountRef.current += 1
    if (dragCountRef.current === 1) setIsDragOver(true)
  }, [])

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    dragCountRef.current -= 1
    if (dragCountRef.current === 0) setIsDragOver(false)
  }, [])

  // markdown formatting
  const applyMdFormat = useCallback((action: MdAction) => {
    const el = mdTextareaRef.current
    if (!el) return
    const start = el.selectionStart
    const end = el.selectionEnd
    const selected = markdownText.slice(start, end)
    const replacement = `${action.prefix}${selected || action.label}${action.suffix}`
    const updated = markdownText.slice(0, start) + replacement + markdownText.slice(end)
    setMarkdownText(updated)
    requestAnimationFrame(() => {
      const selectStart = start + action.prefix.length
      const selectEnd = selectStart + (selected || action.label).length
      el.focus()
      el.setSelectionRange(selectStart, selectEnd)
    })
  }, [markdownText])

  const handleMdKeyDown = useCallback((e: React.KeyboardEvent) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
      e.preventDefault()
      onSubmit()
    }
  }, [onSubmit])

  const hasSignature = signatureEnabled && !!signature?.trim()
  const hasQuoted = !!quotedHtml

  return (
    <div
      className={`relative flex h-full flex-col transition-colors ${
        isDragOver ? 'bg-[var(--color-brand-subtle)]' : ''
      }`}
      onDrop={handleDrop}
      onPaste={handlePaste}
      onDragOver={(e) => e.preventDefault()}
      onDragEnter={handleDragEnter}
      onDragLeave={handleDragLeave}
    >
      {/* toolbar — thin bar at top */}
      <div className="flex shrink-0 items-center border-b border-[var(--color-border-default)]">
        {/* mode toggle */}
        <div className="flex items-center gap-0.5 border-r border-[var(--color-border-default)] px-1.5 py-1">
          {([
            { mode: 'rich' as const, icon: Type, title: 'Rich text' },
            { mode: 'markdown' as const, icon: Code2, title: 'Markdown' },
            { mode: 'preview' as const, icon: Eye, title: 'Preview' },
          ]).map(({ mode: m, icon: Icon, title }) => (
            <button
              key={m}
              type="button"
              onClick={() => switchMode(m)}
              title={title}
              className={`rounded-md p-1 transition-colors ${
                editorMode === m
                  ? 'bg-[var(--color-border-default)] text-[var(--color-text-primary)]'
                  : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-hover)] hover:text-[var(--color-text-secondary)]'
              }`}
            >
              <Icon className="h-3.5 w-3.5" />
            </button>
          ))}
        </div>

        {editorMode === 'rich' && (
          <div className="flex-1 overflow-hidden">
            <EditorToolbar editor={composeEditor} />
          </div>
        )}

        {editorMode === 'markdown' && (
          <div className="flex flex-1 flex-wrap items-center gap-0.5 px-2 py-1">
            {MD_ACTIONS.map((a) => (
              <button
                key={a.label}
                type="button"
                onClick={() => applyMdFormat(a)}
                title={a.label}
                className="rounded-md px-1.5 py-0.5 text-xs text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text-secondary)]"
              >
                {a.icon === 'B' ? <span className="font-bold">{a.icon}</span>
                  : a.icon === 'I' ? <span className="italic">{a.icon}</span>
                  : a.icon === '</>' || a.icon === '{ }' ? <span className="font-mono text-[10px]">{a.icon}</span>
                  : <span className="text-[10px]">{a.icon}</span>
                }
              </button>
            ))}
          </div>
        )}

        {editorMode === 'preview' && (
          <div className="flex-1 px-3 py-1.5">
            <span className="text-xs text-[var(--color-text-tertiary)]">Recipient preview</span>
          </div>
        )}
      </div>

      {/* main content area — fills all remaining space, click anywhere to focus */}
      <div
        ref={scrollAreaRef}
        onClick={handleAreaClick}
        className={`flex min-h-0 flex-1 cursor-text flex-col overflow-y-auto ${disabled ? 'pointer-events-none opacity-50' : ''}`}
      >
        {/* rich mode */}
        {editorMode === 'rich' && (
          <EditorContent editor={composeEditor} />
        )}

        {/* markdown mode */}
        {editorMode === 'markdown' && (
          <textarea
            ref={mdTextareaRef}
            value={markdownText}
            onChange={(e) => setMarkdownText(e.target.value)}
            onKeyDown={handleMdKeyDown}
            placeholder={placeholder ?? 'Write in Markdown...'}
            disabled={disabled}
            className="h-full min-h-[120px] w-full resize-none bg-transparent px-3 py-2 font-mono text-sm leading-relaxed text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)]"
          />
        )}

        {/* preview mode */}
        {editorMode === 'preview' && (
          <div className="cursor-default px-4 py-3">
            <div className="prose prose-sm max-w-none text-[var(--color-text-primary)]">
              {markdownText ? (
                <Markdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
                  {markdownText}
                </Markdown>
              ) : composeEditor ? (
                <div dangerouslySetInnerHTML={{ __html: composeEditor.getHTML() }} />
              ) : null}
            </div>

            {hasSignature && sigEditor?.getText().trim() && (
              <div className="mt-4 border-t border-dashed border-[var(--color-border-default)] pt-2 opacity-60">
                <p className="text-sm text-[var(--color-text-tertiary)]">-- </p>
                <div
                  className="prose prose-sm max-w-none text-[var(--color-text-tertiary)]"
                  dangerouslySetInnerHTML={{ __html: sigEditor.getHTML() }}
                />
              </div>
            )}

            {hasQuoted && quotedEditor?.getText().trim() && (
              <div className="mt-4 border-l-2 border-[var(--color-border-default)] pl-3 opacity-50">
                <div
                  className="prose prose-sm max-w-none text-[var(--color-text-tertiary)]"
                  dangerouslySetInnerHTML={{ __html: quotedEditor.getHTML() }}
                />
              </div>
            )}
          </div>
        )}

        {/* signature zone (rich + markdown modes) */}
        {editorMode !== 'preview' && hasSignature && (
          <div className="cursor-default border-t border-dashed border-[var(--color-border-default)] opacity-60">
            <EditorContent editor={sigEditor} />
          </div>
        )}

        {/* quoted zone (rich + markdown modes) */}
        {editorMode !== 'preview' && hasQuoted && (
          <div className="cursor-default border-t border-[var(--color-border-default)]">
            <button
              type="button"
              onClick={() => setQuotedExpanded((v) => !v)}
              className="flex w-full cursor-pointer items-center gap-1 px-3 py-1.5 text-xs text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)]"
            >
              {quotedExpanded
                ? <ChevronDown className="h-3 w-3" />
                : <ChevronRight className="h-3 w-3" />
              }
              {quotedExpanded ? 'Hide original' : `Show original${mode === 'forward' ? ' (forwarded)' : ''}`}
            </button>
            {quotedExpanded && (
              <div className="border-l-2 border-[var(--color-border-default)] opacity-50">
                <EditorContent editor={quotedEditor} />
              </div>
            )}
          </div>
        )}
      </div>

      {/* drag overlay */}
      {isDragOver && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
          <span className="rounded-full bg-[var(--color-brand-primary)] px-3 py-1 text-xs font-medium text-white shadow-lg">
            Drop image to insert
          </span>
        </div>
      )}
    </div>
  )
})
