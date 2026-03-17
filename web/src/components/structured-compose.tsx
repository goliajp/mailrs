import { useCallback, useEffect, useImperativeHandle, useRef, useState, forwardRef } from 'react'
import { useEditor, EditorContent, type Editor } from '@tiptap/react'
import { ChevronDown, ChevronRight } from 'lucide-react'

import {
  EditorToolbar,
  createEditorExtensions,
  createMinimalExtensions,
  uploadInlineImage,
  PROSE_CLASS,
} from '@/components/rich-editor'

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

function assembleContent(
  compose: Editor | null,
  sig: Editor | null,
  quoted: Editor | null,
  quotedHeader: string,
): StructuredContent {
  const c = compose
    ? { text: compose.getText(), html: compose.getHTML() }
    : { text: '', html: '' }
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

  return {
    compose: c,
    signature: s,
    quoted: q,
    fullText,
    fullHtml,
  }
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
  const [isDragOver, setIsDragOver] = useState(false)
  const [quotedExpanded, setQuotedExpanded] = useState(false)
  const dragCountRef = useRef(0)
  const sigInitializedRef = useRef(false)
  const quotedInitializedRef = useRef(false)

  // compose editor: full capabilities
  const composeEditor = useEditor({
    extensions: createEditorExtensions(placeholder),
    editorProps: {
      attributes: { class: PROSE_CLASS + ' min-h-[3rem]' },
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

  // signature editor: minimal formatting
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
      const html = quotedHeaderHtml
        ? quotedHeaderHtml + quotedHtml
        : quotedHtml
      quotedEditor.commands.setContent(html)
      quotedInitializedRef.current = true
    }
  }, [quotedEditor, quotedHtml, quotedHeaderHtml])

  // reset quoted when content changes (e.g. mode switch)
  useEffect(() => {
    quotedInitializedRef.current = false
  }, [quotedHtml])

  // expose handle
  useImperativeHandle(ref, () => ({
    getContent: () => assembleContent(composeEditor, sigEditor, quotedEditor, quotedHeader),
    getComposeEditor: () => composeEditor,
    clearCompose: () => composeEditor?.commands.clearContent(),
    setComposeContent: (html: string) => composeEditor?.commands.setContent(html),
  }), [composeEditor, sigEditor, quotedEditor, quotedHeader])

  // drag-drop for images
  const handleDrop = useCallback(async (e: React.DragEvent) => {
    setIsDragOver(false)
    dragCountRef.current = 0
    if (!composeEditor) return
    const files = Array.from(e.dataTransfer.files).filter((f) => f.type.startsWith('image/'))
    if (files.length === 0) return
    e.preventDefault()
    for (const file of files) {
      const url = await uploadInlineImage(file)
      if (url) composeEditor.chain().focus().setImage({ src: url }).run()
    }
  }, [composeEditor])

  const handlePaste = useCallback(async (e: React.ClipboardEvent) => {
    if (!composeEditor) return
    const items = Array.from(e.clipboardData.items).filter((i) => i.type.startsWith('image/'))
    if (items.length === 0) return
    e.preventDefault()
    for (const item of items) {
      const file = item.getAsFile()
      if (!file) continue
      const url = await uploadInlineImage(file)
      if (url) composeEditor.chain().focus().setImage({ src: url }).run()
    }
  }, [composeEditor])

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

  const hasSignature = signatureEnabled && !!signature?.trim()
  const hasQuoted = !!quotedHtml

  return (
    <div
      className={`relative flex h-full flex-col rounded-lg border transition-colors ${
        isDragOver
          ? 'border-[var(--color-brand-primary)] bg-[var(--color-brand-subtle)]'
          : 'border-[var(--color-border-default)] bg-[var(--color-bg-sunken)]'
      }`}
      onDrop={handleDrop}
      onPaste={handlePaste}
      onDragOver={(e) => e.preventDefault()}
      onDragEnter={handleDragEnter}
      onDragLeave={handleDragLeave}
    >
      {/* toolbar — bound to compose editor */}
      <EditorToolbar editor={composeEditor} />

      {/* scrollable content area containing all zones */}
      <div className={`min-h-0 flex-1 overflow-y-auto ${disabled ? 'pointer-events-none opacity-50' : ''}`}>
        {/* compose zone */}
        <EditorContent editor={composeEditor} />

        {/* signature zone */}
        {hasSignature && (
          <div className="border-t border-dashed border-[var(--color-border-default)] opacity-60">
            <EditorContent editor={sigEditor} />
          </div>
        )}

        {/* quoted zone */}
        {hasQuoted && (
          <div className="border-t border-[var(--color-border-default)]">
            <button
              type="button"
              onClick={() => setQuotedExpanded((v) => !v)}
              className="flex w-full items-center gap-1 px-3 py-1.5 text-xs text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)]"
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
