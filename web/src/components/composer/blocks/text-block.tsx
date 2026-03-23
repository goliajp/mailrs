import { useCallback, useEffect, useRef } from 'react'
import { useEditor, EditorContent, type Editor } from '@tiptap/react'

import {
  EditorToolbar,
  createEditorExtensions,
  uploadInlineImage,
  PROSE_CLASS,
} from '@/components/rich-editor'
import type { TextBlockData } from '../types'

type Props = {
  data: TextBlockData
  onChange: (data: TextBlockData) => void
  onSubmit: () => void
  disabled?: boolean
  placeholder?: string
  getEditorRef?: (editor: Editor | null) => void
}

export function TextBlock({ onChange, onSubmit, disabled, placeholder, getEditorRef }: Props) {
  const onSubmitRef = useRef(onSubmit)
  useEffect(() => {
    onSubmitRef.current = onSubmit
  }, [onSubmit])

  const editor = useEditor({
    extensions: createEditorExtensions(placeholder),
    editorProps: {
      attributes: { class: PROSE_CLASS + ' min-h-[3rem]', style: 'cursor:text' },
      handleKeyDown: (_view, event) => {
        if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') {
          event.preventDefault()
          onSubmitRef.current()
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

  // image drag-drop
  const handleDrop = useCallback(
    async (e: React.DragEvent) => {
      if (!editor) return
      const files = Array.from(e.dataTransfer.files).filter((f) => f.type.startsWith('image/'))
      if (files.length === 0) return
      e.preventDefault()
      for (const file of files) {
        const url = await uploadInlineImage(file)
        if (url) editor.chain().focus().setImage({ src: url }).run()
      }
    },
    [editor],
  )

  const handlePaste = useCallback(
    async (e: React.ClipboardEvent) => {
      if (!editor) return
      const items = Array.from(e.clipboardData.items).filter((i) => i.type.startsWith('image/'))
      if (items.length === 0) return
      e.preventDefault()
      for (const item of items) {
        const file = item.getAsFile()
        if (!file) continue
        const url = await uploadInlineImage(file)
        if (url) editor.chain().focus().setImage({ src: url }).run()
      }
    },
    [editor],
  )

  return (
    <div onDrop={handleDrop} onPaste={handlePaste} onDragOver={(e) => e.preventDefault()}>
      <div className="border-b border-[var(--color-border-default)]">
        <EditorToolbar editor={editor} />
      </div>
      <div className={`flex-1 cursor-text ${disabled ? 'pointer-events-none opacity-50' : ''}`}>
        <EditorContent editor={editor} />
      </div>
    </div>
  )
}
