import { useCallback, useImperativeHandle, useRef, useState, forwardRef, type ReactNode } from 'react'
import type { Editor } from '@tiptap/react'
import { X } from 'lucide-react'

import { useBlockComposer } from '@/components/composer/use-block-composer'
import { TextBlock } from '@/components/composer/blocks/text-block'
import { SignatureBlock } from '@/components/composer/blocks/signature-block'
import { QuoteBlock } from '@/components/composer/blocks/quote-block'
import { DividerBlock } from '@/components/composer/blocks/divider-block'
import { AttachmentBlock } from '@/components/composer/blocks/attachment-block'
import { AddBlockMenu } from '@/components/composer/add-block-menu'
import type { TextBlockData, SignatureBlockData, QuoteBlockData, AttachmentBlockData, AnyBlock } from '@/components/composer/types'

// keep backward-compatible types
export type EditorMode = 'rich' | 'markdown' | 'preview'

export type StructuredContent = {
  compose: { text: string; html: string }
  signature: { text: string; html: string }
  quoted: { text: string; html: string }
  fullText: string
  fullHtml: string
  attachments: File[]
}

export type StructuredComposeHandle = {
  getContent: () => StructuredContent
  getComposeEditor: () => Editor | null
  clearCompose: () => void
  setComposeContent: (html: string) => void
  getEditorMode: () => EditorMode
  addAttachment: (file: File) => void
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
  const fileInputRef = useRef<HTMLInputElement>(null)
  const textEditorRef = useRef<Editor | null>(null)
  const scrollAreaRef = useRef<HTMLDivElement>(null)

  const {
    blocks,
    addBlock,
    addAttachment,
    removeBlock,
    updateBlock,
    clearCompose,
    getAssembled,
  } = useBlockComposer({
    signature,
    signatureEnabled,
    quotedHtml,
    quotedHeader,
    quotedHeaderHtml,
    mode: mode ?? 'new',
  })

  const setTextEditorRef = useCallback((editor: Editor | null) => {
    textEditorRef.current = editor
  }, [])

  // backward-compatible handle
  useImperativeHandle(ref, () => ({
    getContent: () => {
      const assembled = getAssembled()
      // extract parts for backward compat
      const textBlock = blocks.find((b) => b.type === 'text')
      const sigBlock = blocks.find((b) => b.type === 'signature')
      const quoteBlock = blocks.find((b) => b.type === 'quote')
      const attachments = blocks
        .filter((b) => b.type === 'attachment')
        .map((b) => (b.data as AttachmentBlockData).file)
        .filter((f): f is File => f != null)

      return {
        compose: textBlock
          ? { text: (textBlock.data as TextBlockData).content, html: (textBlock.data as TextBlockData).html }
          : { text: '', html: '' },
        signature: sigBlock
          ? { text: (sigBlock.data as SignatureBlockData).text, html: (sigBlock.data as SignatureBlockData).html }
          : { text: '', html: '' },
        quoted: quoteBlock
          ? { text: '', html: (quoteBlock.data as QuoteBlockData).html }
          : { text: '', html: '' },
        fullText: assembled.text,
        fullHtml: assembled.html,
        attachments,
      }
    },
    getComposeEditor: () => textEditorRef.current,
    clearCompose,
    setComposeContent: (html: string) => {
      textEditorRef.current?.commands.setContent(html)
    },
    getEditorMode: () => {
      const textBlock = blocks.find((b) => b.type === 'text')
      return textBlock ? (textBlock.data as TextBlockData).format : 'rich'
    },
    addAttachment,
  }), [blocks, getAssembled, clearCompose, addAttachment])

  // click empty space → focus first text editor
  const handleAreaClick = useCallback((e: React.MouseEvent<HTMLDivElement>) => {
    if (e.target !== scrollAreaRef.current) return
    textEditorRef.current?.commands.focus('end')
  }, [])

  const handleFileSelect = useCallback(() => {
    if (fileInputRef.current) {
      fileInputRef.current.value = ''
      fileInputRef.current.click()
    }
  }, [])

  const handleFilesAdded = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const selected = Array.from(e.target.files ?? [])
    for (const file of selected) addAttachment(file)
    e.target.value = ''
  }, [addAttachment])

  // drag-and-drop attachment support
  const [dragging, setDragging] = useState(false)
  const dragCounter = useRef(0)
  const handleDragEnter = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    dragCounter.current++
    if (e.dataTransfer.types.includes('Files')) setDragging(true)
  }, [])
  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    dragCounter.current--
    if (dragCounter.current === 0) setDragging(false)
  }, [])
  const handleDragOver = useCallback((e: React.DragEvent) => { e.preventDefault() }, [])
  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    dragCounter.current = 0
    setDragging(false)
    const files = Array.from(e.dataTransfer.files)
    for (const file of files) addAttachment(file)
  }, [addAttachment])

  // wrapper that shows a delete button on hover for removable blocks
  const Removable = useCallback(({ id, children, className = '' }: { id: string; children: ReactNode; className?: string }) => (
    <div className={`group relative ${className}`}>
      {children}
      <button
        type="button"
        onClick={() => removeBlock(id)}
        className="absolute right-2 top-1 z-10 rounded-full bg-[var(--color-bg-overlay)] p-0.5 text-[var(--color-text-tertiary)] opacity-0 shadow-sm transition-opacity hover:bg-[var(--color-status-danger-subtle)] hover:text-[var(--color-status-danger)] group-hover:opacity-100"
        aria-label="Remove block"
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  ), [removeBlock])

  const renderBlock = (block: AnyBlock, index: number) => {
    const key = block.id
    // first text block and auto-managed blocks (signature, quote) are not removable
    const isFirstText = block.type === 'text' && index === 0
    const isAutoManaged = block.type === 'signature' || block.type === 'quote'
    const canRemove = !isFirstText && !isAutoManaged

    switch (block.type) {
      case 'text':
        return canRemove ? (
          <Removable key={key} id={block.id}>
            <TextBlock
              data={block.data as TextBlockData}
              onChange={(data) => updateBlock(block.id, data)}
              onSubmit={onSubmit}
              disabled={disabled}
              placeholder="Continue writing..."
              />
          </Removable>
        ) : (
          <TextBlock
            key={key}
            data={block.data as TextBlockData}
            onChange={(data) => updateBlock(block.id, data)}
            onSubmit={onSubmit}
            disabled={disabled}
            placeholder={placeholder}
            getEditorRef={setTextEditorRef}
          />
        )

      case 'signature':
        return (
          <SignatureBlock
            key={key}
            data={block.data as SignatureBlockData}
            onChange={(data) => updateBlock(block.id, data)}
            disabled={disabled}
          />
        )

      case 'quote':
        return (
          <QuoteBlock
            key={key}
            data={block.data as QuoteBlockData}
            onChange={(data) => updateBlock(block.id, data)}
            mode={mode === 'forward' ? 'forward' : 'reply'}
          />
        )

      case 'divider':
        return (
          <div key={key} className="group relative flex items-center px-4">
            <div className="flex-1"><DividerBlock /></div>
            <button
              type="button"
              onClick={() => removeBlock(block.id)}
              className="ml-2 shrink-0 rounded-full bg-[var(--color-bg-overlay)] p-0.5 text-[var(--color-text-tertiary)] opacity-0 shadow-sm transition-opacity hover:bg-[var(--color-status-danger-subtle)] hover:text-[var(--color-status-danger)] group-hover:opacity-100"
              aria-label="Remove divider"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
        )

      case 'attachment':
        return (
          <div key={key} className="px-4 py-1">
            <AttachmentBlock
              data={block.data as AttachmentBlockData}
              onRemove={() => removeBlock(block.id)}
            />
          </div>
        )

      default:
        return null
    }
  }

  return (
    <div className="relative flex h-full flex-col"
      onDragEnter={handleDragEnter}
      onDragLeave={handleDragLeave}
      onDragOver={handleDragOver}
      onDrop={handleDrop}
    >
      {dragging && (
        <div className="absolute inset-0 z-10 flex items-center justify-center rounded-lg border-2 border-dashed border-[var(--color-brand-primary)] bg-[var(--color-brand-subtle)]">
          <p className="text-sm font-medium text-[var(--color-brand-primary)]">Drop files to attach</p>
        </div>
      )}
      {/* block content area */}
      <div
        ref={scrollAreaRef}
        onClick={handleAreaClick}
        className={`flex min-h-0 flex-1 cursor-text flex-col overflow-y-auto overflow-x-hidden ${disabled ? 'pointer-events-none opacity-50' : ''}`}
      >
        {blocks.map((block, i) => renderBlock(block, i))}
      </div>

      {/* add block bar */}
      <div className="flex shrink-0 items-center border-t border-[var(--color-border-default)] px-4 py-1.5">
        <AddBlockMenu
          onAdd={(type) => addBlock(type)}
          onAddFile={handleFileSelect}
        />
        <input
          ref={fileInputRef}
          type="file"
          multiple
          className="hidden"
          onChange={handleFilesAdded}
        />
      </div>
    </div>
  )
})
