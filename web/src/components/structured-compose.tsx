import type {
  AnyBlock,
  AttachmentBlockData,
  QuoteBlockData,
  SignatureBlockData,
  TextBlockData,
} from '@/components/composer/types'
import type { Editor } from '@tiptap/react'

import { X } from 'lucide-react'
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

// keep backward-compatible types
export type EditorMode = 'markdown' | 'preview' | 'rich'

export type StructuredComposeHandle = {
  addAttachment: (file: File) => void
  clearCompose: () => void
  getComposeEditor: () => Editor | null
  getContent: () => StructuredContent
  getEditorMode: () => EditorMode
  setComposeContent: (html: string) => void
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
    const textEditorRef = useRef<Editor | null>(null)
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

    const setTextEditorRef = useCallback((editor: Editor | null) => {
      textEditorRef.current = editor
    }, [])

    // backward-compatible handle
    useImperativeHandle(
      ref,
      () => ({
        addAttachment,
        clearCompose,
        getComposeEditor: () => textEditorRef.current,
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
            attachments,
            compose: textBlock
              ? {
                  html: (textBlock.data as TextBlockData).html,
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
        getEditorMode: () => {
          const textBlock = blocks.find((b) => b.type === 'text')
          return textBlock ? (textBlock.data as TextBlockData).format : 'rich'
        },
        setComposeContent: (html: string) => {
          textEditorRef.current?.commands.setContent(html)
        },
      }),
      [blocks, getAssembled, clearCompose, addAttachment]
    )

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

    const handleFilesAdded = useCallback(
      (e: React.ChangeEvent<HTMLInputElement>) => {
        const selected = Array.from(e.target.files ?? [])
        for (const file of selected) addAttachment(file)
        e.target.value = ''
      },
      [addAttachment]
    )

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
      // first text block and auto-managed blocks (signature, quote) are not removable
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
                placeholder="Continue writing..."
              />
            </Removable>
          ) : (
            <TextBlock
              data={block.data as TextBlockData}
              disabled={disabled}
              getEditorRef={setTextEditorRef}
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
