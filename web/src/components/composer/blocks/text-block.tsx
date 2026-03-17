import { useCallback, useEffect, useRef, useState } from 'react'
import { useEditor, EditorContent, type Editor } from '@tiptap/react'
import { Code2, Type } from 'lucide-react'
import { marked } from 'marked'
import { EditorView, keymap, placeholder as cmPlaceholder } from '@codemirror/view'
import { EditorState } from '@codemirror/state'
import { markdown } from '@codemirror/lang-markdown'
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands'
import { oneDark } from '@codemirror/theme-one-dark'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeHighlight from 'rehype-highlight'

import {
  EditorToolbar,
  createEditorExtensions,
  uploadInlineImage,
  PROSE_CLASS,
} from '@/components/rich-editor'
import type { TextBlockData } from '../types'

const cmTheme = EditorView.theme({
  '&': { height: '100%', fontSize: '13px' },
  '.cm-content': { fontFamily: '"SF Mono", Monaco, Consolas, monospace', padding: '8px 12px', caretColor: 'var(--color-text-primary)' },
  '.cm-line': { padding: '0' },
  '.cm-gutters': { display: 'none' },
  '.cm-scroller': { overflow: 'auto' },
  '.cm-focused': { outline: 'none' },
  '&.cm-focused .cm-cursor': { borderLeftColor: 'var(--color-text-primary)' },
  '.cm-placeholder': { color: 'var(--color-text-tertiary)' },
})

type Props = {
  data: TextBlockData
  onChange: (data: TextBlockData) => void
  onSubmit: () => void
  disabled?: boolean
  placeholder?: string
  getEditorRef?: (editor: Editor | null) => void
}

export function TextBlock({ data, onChange, onSubmit, disabled, placeholder, getEditorRef }: Props) {
  const [format, setFormat] = useState<'rich' | 'markdown'>(data.format)
  const [mdText, setMdText] = useState(data.content)
  const cmContainerRef = useRef<HTMLDivElement>(null)
  const cmViewRef = useRef<EditorView | null>(null)
  const onSubmitRef = useRef(onSubmit)
  onSubmitRef.current = onSubmit

  const editor = useEditor({
    extensions: createEditorExtensions(placeholder),
    editorProps: {
      attributes: { class: PROSE_CLASS + ' min-h-[3rem]', style: 'cursor:text' },
      handleKeyDown: (_view, event) => {
        if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') {
          event.preventDefault()
          onSubmit()
          return true
        }
        if (event.key === 'Tab' && editor?.isActive('codeBlock')) {
          event.preventDefault()
          if (!event.shiftKey) editor?.commands.insertContent('  ')
          return true
        }
        return false
      },
    },
    editable: !disabled,
    onUpdate: ({ editor: e }) => {
      onChange({ content: e.getText(), html: e.getHTML(), format: 'rich' })
    },
  })

  useEffect(() => {
    if (editor && getEditorRef) getEditorRef(editor)
  }, [editor, getEditorRef])

  // init codemirror
  useEffect(() => {
    if (format !== 'markdown' || !cmContainerRef.current || cmViewRef.current) return

    const view = new EditorView({
      state: EditorState.create({
        doc: mdText,
        extensions: [
          markdown(),
          history(),
          keymap.of([{ key: 'Mod-Enter', run: () => { onSubmitRef.current(); return true } }]),
          keymap.of([...defaultKeymap, ...historyKeymap]),
          cmPlaceholder(placeholder ?? 'Write in Markdown...'),
          oneDark,
          cmTheme,
          EditorView.lineWrapping,
          EditorView.updateListener.of((update) => {
            if (update.docChanged) {
              const text = update.state.doc.toString()
              setMdText(text)
              onChange({
                content: text,
                html: marked.parse(text, { async: false, gfm: true, breaks: true }) as string,
                format: 'markdown',
              })
            }
          }),
        ],
      }),
      parent: cmContainerRef.current,
    })
    cmViewRef.current = view
    view.focus()
    return () => { view.destroy(); cmViewRef.current = null }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [format])

  const switchFormat = useCallback((newFormat: 'rich' | 'markdown') => {
    if (newFormat === format) return
    if (format === 'rich' && newFormat === 'markdown') {
      setMdText(editor?.getText() ?? '')
    } else if (format === 'markdown' && newFormat === 'rich') {
      const html = marked.parse(mdText, { async: false, gfm: true, breaks: true }) as string
      editor?.commands.setContent(html)
    }
    if (cmViewRef.current) { cmViewRef.current.destroy(); cmViewRef.current = null }
    setFormat(newFormat)
  }, [format, editor, mdText])

  // image drag-drop (rich mode)
  const handleDrop = useCallback(async (e: React.DragEvent) => {
    if (!editor || format !== 'rich') return
    const files = Array.from(e.dataTransfer.files).filter((f) => f.type.startsWith('image/'))
    if (files.length === 0) return
    e.preventDefault()
    for (const file of files) {
      const url = await uploadInlineImage(file)
      if (url) editor.chain().focus().setImage({ src: url }).run()
    }
  }, [editor, format])

  const handlePaste = useCallback(async (e: React.ClipboardEvent) => {
    if (!editor || format !== 'rich') return
    const items = Array.from(e.clipboardData.items).filter((i) => i.type.startsWith('image/'))
    if (items.length === 0) return
    e.preventDefault()
    for (const item of items) {
      const file = item.getAsFile()
      if (!file) continue
      const url = await uploadInlineImage(file)
      if (url) editor.chain().focus().setImage({ src: url }).run()
    }
  }, [editor, format])

  return (
    <div onDrop={handleDrop} onPaste={handlePaste} onDragOver={(e) => e.preventDefault()}>
      {/* format toggle + toolbar */}
      <div className="flex items-center border-b border-[var(--color-border-default)]">
        <div className="flex items-center gap-0.5 border-r border-[var(--color-border-default)] px-1.5 py-1">
          <button type="button" onClick={() => switchFormat('rich')} title="Rich text"
            className={`rounded-md p-1 transition-colors ${format === 'rich' ? 'bg-[var(--color-border-default)] text-[var(--color-text-primary)]' : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-hover)]'}`}>
            <Type className="h-3.5 w-3.5" />
          </button>
          <button type="button" onClick={() => switchFormat('markdown')} title="Markdown"
            className={`rounded-md p-1 transition-colors ${format === 'markdown' ? 'bg-[var(--color-border-default)] text-[var(--color-text-primary)]' : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-hover)]'}`}>
            <Code2 className="h-3.5 w-3.5" />
          </button>
        </div>
        {format === 'rich' && (
          <div className="flex-1 overflow-hidden">
            <EditorToolbar editor={editor} />
          </div>
        )}
        {format === 'markdown' && (
          <div className="flex-1 px-3 py-1.5">
            <span className="text-xs text-[var(--color-text-tertiary)]">Source | Preview</span>
          </div>
        )}
      </div>

      {/* content */}
      {format === 'rich' && (
        <div className={`flex-1 cursor-text ${disabled ? 'pointer-events-none opacity-50' : ''}`}>
          <EditorContent editor={editor} />
        </div>
      )}

      {format === 'markdown' && (
        <div className="flex min-h-[120px]">
          <div ref={cmContainerRef} className="flex-1 overflow-y-auto border-r border-[var(--color-border-default)]" />
          <div className="flex-1 overflow-y-auto bg-[var(--color-bg-base)] px-4 py-3">
            <div className="prose prose-sm max-w-none text-[var(--color-text-primary)]">
              {mdText.trim() ? (
                <Markdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>{mdText}</Markdown>
              ) : (
                <p className="text-[var(--color-text-tertiary)]">Preview...</p>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
