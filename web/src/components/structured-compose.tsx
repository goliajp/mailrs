import type {
  AnyBlock,
  AttachmentBlockData,
  QuoteBlockData,
  SignatureBlockData,
  TextBlockData,
} from '@/components/composer/types'

import { X } from 'lucide-react'
import { marked } from 'marked'
import {
  forwardRef,
  type ReactNode,
  useCallback,
  useImperativeHandle,
  useRef,
  useState,
} from 'react'

import { AddBlockMenu } from '@/components/composer/add-block-menu'
import { AttachmentBlock } from '@/components/composer/blocks/attachment-block'
import { DividerBlock } from '@/components/composer/blocks/divider-block'
import { QuoteBlock } from '@/components/composer/blocks/quote-block'
import { SignatureBlock } from '@/components/composer/blocks/signature-block'
import { TextBlock } from '@/components/composer/blocks/text-block'
import { useBlockComposer } from '@/components/composer/use-block-composer'

// keep backward-compatible type; rich is gone, markdown is the only authoring mode
export type EditorMode = 'markdown' | 'preview'

export type StructuredComposeHandle = {
  addAttachment: (file: File) => void
  clearCompose: () => void
  /** @deprecated tiptap editor removed; always returns null. use getMarkdown / setMarkdown */
  getComposeEditor: () => null
  getContent: () => StructuredContent
  getEditorMode: () => EditorMode
  getMarkdown: () => string
  /** @deprecated use setMarkdown; html input is stripped to text */
  setComposeContent: (html: string) => void
  setMarkdown: (markdown: string) => void
}

export type StructuredContent = {
  attachments: File[]
  compose: { html: string; text: string }
  fullHtml: string
  fullText: string
  quoted: { html: string; text: string }
  signature: { html: string; text: string }
}

type Props = {
  disabled?: boolean
  mode?: 'forward' | 'new' | 'reply'
  onSubmit: () => void
  placeholder?: string
  quotedHeader?: string
  quotedHeaderHtml?: string
  quotedHtml?: string
  signature?: string
  signatureEnabled?: boolean
}

export const StructuredCompose = forwardRef<StructuredComposeHandle, Props>(
  function StructuredCompose(
    {
      disabled,
      mode = 'new',
      onSubmit,
      placeholder,
      quotedHeader = '',
      quotedHeaderHtml,
      quotedHtml,
      signature,
      signatureEnabled,
    },
    ref
  ) {
    const fileInputRef = useRef<HTMLInputElement>(null)
    const scrollAreaRef = useRef<HTMLDivElement>(null)

    const {
      addAttachment,
      addBlock,
      blocks,
      clearCompose,
      getAssembled,
      removeBlock,
      updateBlock,
    } = useBlockComposer({
      mode: mode ?? 'new',
      quotedHeader,
      quotedHeaderHtml,
      quotedHtml,
      signature,
      signatureEnabled,
    })

    // first text block is the user-authored body. helpers below operate on it.
    const blocksRef = useRef(blocks)
    blocksRef.current = blocks

    const firstTextBlock = useCallback(() => {
      return blocksRef.current.find((b) => b.type === 'text')
    }, [])

    useImperativeHandle(
      ref,
      () => ({
        addAttachment,
        clearCompose,
        getComposeEditor: () => null,
        getContent: () => {
          const assembled = getAssembled()
          const textBlock = blocks.find((b) => b.type === 'text')
          const sigBlock = blocks.find((b) => b.type === 'signature')
          const quoteBlock = blocks.find((b) => b.type === 'quote')
          const attachments = blocks
            .filter((b) => b.type === 'attachment')
            .map((b) => (b.data as AttachmentBlockData).file)
            .filter((f): f is File => f != null)

          return {
            attachments,
            compose: textBlock
              ? {
                  // markdown source is authoritative; html is rendered on demand
                  // so callers (e.g. backend-forward path) get the user's body
                  // alone, without signature/quote.
                  html: renderTextBlockHtml(textBlock.data as TextBlockData),
                  text: (textBlock.data as TextBlockData).content,
                }
              : { html: '', text: '' },
            fullHtml: assembled.html,
            fullText: assembled.text,
            quoted: quoteBlock
              ? { html: (quoteBlock.data as QuoteBlockData).html, text: '' }
              : { html: '', text: '' },
            signature: sigBlock
              ? {
                  html: (sigBlock.data as SignatureBlockData).html,
                  text: (sigBlock.data as SignatureBlockData).text,
                }
              : { html: '', text: '' },
          }
        },
        getEditorMode: () => 'markdown',
        getMarkdown: () => {
          const tb = firstTextBlock()
          return tb ? (tb.data as TextBlockData).content : ''
        },
        setComposeContent: (html: string) => {
          const md = htmlToText(html)
          const tb = firstTextBlock()
          if (!tb) return
          updateBlock(tb.id, { content: md, format: 'markdown', html: '' })
        },
        setMarkdown: (markdown: string) => {
          const tb = firstTextBlock()
          if (!tb) return
          updateBlock(tb.id, { content: markdown, format: 'markdown', html: '' })
        },
      }),
      [addAttachment, blocks, clearCompose, firstTextBlock, getAssembled, updateBlock]
    )

    // click empty space → focus the textarea inside the first text block
    const handleAreaClick = useCallback((e: React.MouseEvent<HTMLDivElement>) => {
      if (e.target !== scrollAreaRef.current) return
      const ta = scrollAreaRef.current.querySelector<HTMLTextAreaElement>('textarea')
      ta?.focus()
    }, [])

    const handleFileSelect = useCallback(() => {
      if (fileInputRef.current) {
        fileInputRef.current.value = ''
        fileInputRef.current.click()
      }
    }, [])

    const handleFilesAdded = useCallback(
      (e: React.ChangeEvent<HTMLInputElement>) => {
        const selected = Array.from(e.target.files ?? [])
        for (const file of selected) addAttachment(file)
        e.target.value = ''
      },
      [addAttachment]
    )

    // drag-and-drop attachment support (non-image files; image drops inside
    // the textarea convert to markdown and stop propagation, so they never
    // reach this handler)
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
    const handleDragOver = useCallback((e: React.DragEvent) => {
      e.preventDefault()
    }, [])
    const handleDrop = useCallback(
      (e: React.DragEvent) => {
        e.preventDefault()
        dragCounter.current = 0
        setDragging(false)
        const files = Array.from(e.dataTransfer.files)
        for (const file of files) addAttachment(file)
      },
      [addAttachment]
    )

    // wrapper that shows a delete button on hover for removable blocks
    const Removable = useCallback(
      ({
        children,
        className = '',
        id,
      }: {
        children: ReactNode
        className?: string
        id: string
      }) => (
        <div className={`group relative ${className}`}>
          {children}
          <button
            aria-label="Remove block"
            className="touch-target bg-surface text-fg-muted hover:bg-danger/10 hover:text-danger absolute top-1 right-2 z-10 rounded-full p-1 opacity-100 shadow-sm transition-opacity md:p-0.5 md:opacity-0 md:group-hover:opacity-100"
            onClick={() => removeBlock(id)}
            type="button"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
      ),
      [removeBlock]
    )

    const renderBlock = (block: AnyBlock, index: number) => {
      const key = block.id
      const isFirstText = block.type === 'text' && index === 0
      const isAutoManaged = block.type === 'signature' || block.type === 'quote'
      const canRemove = !isFirstText && !isAutoManaged

      switch (block.type) {
        case 'attachment':
          return (
            <div className="px-4 py-1" key={key}>
              <AttachmentBlock
                data={block.data as AttachmentBlockData}
                onRemove={() => removeBlock(block.id)}
              />
            </div>
          )

        case 'divider':
          return (
            <div className="group relative flex items-center px-4" key={key}>
              <div className="flex-1">
                <DividerBlock />
              </div>
              <button
                aria-label="Remove divider"
                className="touch-target bg-surface text-fg-muted hover:bg-danger/10 hover:text-danger ml-2 shrink-0 rounded-full p-1 opacity-100 shadow-sm transition-opacity md:p-0.5 md:opacity-0 md:group-hover:opacity-100"
                onClick={() => removeBlock(block.id)}
                type="button"
              >
                <X className="h-3.5 w-3.5" />
              </button>
            </div>
          )

        case 'quote':
          return (
            <QuoteBlock
              data={block.data as QuoteBlockData}
              key={key}
              mode={mode === 'forward' ? 'forward' : 'reply'}
              onChange={(data) => updateBlock(block.id, data)}
            />
          )

        case 'signature':
          return (
            <SignatureBlock
              data={block.data as SignatureBlockData}
              disabled={disabled}
              key={key}
              onChange={(data) => updateBlock(block.id, data)}
            />
          )

        case 'text':
          return canRemove ? (
            <Removable id={block.id} key={key}>
              <TextBlock
                data={block.data as TextBlockData}
                disabled={disabled}
                onChange={(data) => updateBlock(block.id, data)}
                onSubmit={onSubmit}
                placeholder="Continue writing…"
              />
            </Removable>
          ) : (
            <TextBlock
              data={block.data as TextBlockData}
              disabled={disabled}
              key={key}
              onChange={(data) => updateBlock(block.id, data)}
              onSubmit={onSubmit}
              placeholder={placeholder}
            />
          )

        default:
          return null
      }
    }

    return (
      <div
        className="relative flex h-full flex-col"
        onDragEnter={handleDragEnter}
        onDragLeave={handleDragLeave}
        onDragOver={handleDragOver}
        onDrop={handleDrop}
      >
        {dragging && (
          <div className="border-accent bg-accent/10 absolute inset-0 z-10 flex items-center justify-center rounded-lg border-2 border-dashed">
            <p className="text-accent text-sm font-medium">Drop files to attach</p>
          </div>
        )}
        {/* block content area */}
        <div
          className={`flex min-h-0 flex-1 cursor-text flex-col overflow-x-hidden overflow-y-auto ${disabled ? 'pointer-events-none opacity-50' : ''}`}
          onClick={handleAreaClick}
          ref={scrollAreaRef}
        >
          {blocks.map((block, i) => renderBlock(block, i))}
        </div>

        {/* add block bar */}
        <div className="border-border flex shrink-0 items-center border-t px-4 py-1.5">
          <AddBlockMenu onAdd={(type) => addBlock(type)} onAddFile={handleFileSelect} />
          <input
            className="hidden"
            multiple
            onChange={handleFilesAdded}
            ref={fileInputRef}
            type="file"
          />
        </div>
      </div>
    )
  }
)

// best-effort html → plaintext used by the legacy setComposeContent path
function htmlToText(html: string): string {
  if (typeof window === 'undefined' || !html) return html ?? ''
  const doc = new DOMParser().parseFromString(html, 'text/html')
  for (const br of Array.from(doc.querySelectorAll('br'))) {
    br.replaceWith('\n')
  }
  for (const block of Array.from(doc.querySelectorAll('p, div, li, h1, h2, h3, h4, h5, h6'))) {
    block.append('\n')
  }
  const text = doc.body.textContent ?? ''
  return text.replace(/\n{3,}/g, '\n\n').trim()
}

function renderTextBlockHtml(data: TextBlockData): string {
  if (data.format === 'markdown') {
    return marked.parse(data.content, { async: false, breaks: true, gfm: true }) as string
  }
  return data.html
}
