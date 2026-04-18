import type { TextBlockData } from '../types'

import { type Editor, EditorContent, useEditor } from '@tiptap/react'
import { useCallback, useEffect, useRef } from 'react'

import {
  createEditorExtensions,
  EditorToolbar,
  PROSE_CLASS,
  uploadInlineImage,
} from '@/components/rich-editor'

type Props = {
  data: TextBlockData
  disabled?: boolean
  getEditorRef?: (editor: Editor | null) => void
  onChange: (data: TextBlockData) => void
  onSubmit: () => void
  placeholder?: string
}

export function TextBlock({ disabled, getEditorRef, onChange, onSubmit, placeholder }: Props) {
  const onSubmitRef = useRef(onSubmit)
  useEffect(() => {
    onSubmitRef.current = onSubmit
  }, [onSubmit])

  const editor = useEditor({
    editable: !disabled,
    editorProps: {
      attributes: {
        // give the text block a comfortable minimum height so the empty
        // state doesn't leave a cavernous gap between the placeholder and
        // the next block (signature / quoted original)
        class: PROSE_CLASS + ' min-h-[16rem]',
        style: 'cursor:text',
      },
      handleKeyDown: (_view, event) => {
        if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') {
          event.preventDefault()
          onSubmitRef.current()
          return true
        }
        return false
      },
    },
    extensions: createEditorExtensions(placeholder),
    onUpdate: ({ editor: e }) => {
      onChange({ content: e.getText(), format: 'rich', html: e.getHTML() })
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
    [editor]
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
    [editor]
  )

  return (
    <div onDragOver={(e) => e.preventDefault()} onDrop={handleDrop} onPaste={handlePaste}>
      <div className="border-border border-b">
        <EditorToolbar editor={editor} />
      </div>
      <div className={`flex-1 cursor-text ${disabled ? 'pointer-events-none opacity-50' : ''}`}>
        <EditorContent editor={editor} />
      </div>
    </div>
  )
}
