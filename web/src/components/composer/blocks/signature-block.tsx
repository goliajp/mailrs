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
          'prose prose-sm max-w-none px-3 py-1.5 outline-none text-[var(--color-text-tertiary)]',
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
    <div className="border-t border-dashed border-[var(--color-border-default)] opacity-70">
      <div className="px-4 pt-2 text-xs text-[var(--color-text-tertiary)]">
        --{' '}
      </div>
      <EditorContent editor={editor} />
    </div>
  )
}
