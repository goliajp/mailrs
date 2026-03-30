import type { SignatureBlockData } from '../types'

import { EditorContent, useEditor } from '@tiptap/react'
import { useEffect, useRef } from 'react'

import { createMinimalExtensions } from '@/components/rich-editor'

type Props = {
  data: SignatureBlockData
  disabled?: boolean
  onChange: (data: SignatureBlockData) => void
}

export function SignatureBlock({ data, disabled, onChange }: Props) {
  const initializedRef = useRef(false)

  const editor = useEditor({
    editable: !disabled,
    editorProps: {
      attributes: {
        class:
          'prose prose-sm max-w-none px-3 py-1.5 outline-none text-fg-muted',
      },
    },
    extensions: createMinimalExtensions(),
    onUpdate: ({ editor: e }) => {
      onChange({ html: e.getHTML(), text: e.getText() })
    },
  })

  useEffect(() => {
    if (!editor || initializedRef.current) return
    if (data.html) {
      editor.commands.setContent(data.html)
      initializedRef.current = true
    }
  }, [editor, data.html])

  return (
    <div className="border-border border-t border-dashed opacity-70">
      <div className="text-fg-muted px-4 pt-2 text-xs">-- </div>
      <EditorContent editor={editor} />
    </div>
  )
}
