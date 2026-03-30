import type { QuoteBlockData } from '../types'

import { EditorContent, useEditor } from '@tiptap/react'
import { ChevronDown, ChevronRight } from 'lucide-react'
import { useEffect, useRef } from 'react'

import { createMinimalExtensions } from '@/components/rich-editor'

type Props = {
  data: QuoteBlockData
  mode?: 'forward' | 'reply'
  onChange: (data: QuoteBlockData) => void
}

export function QuoteBlock({ data, mode, onChange }: Props) {
  const initializedRef = useRef(false)
  const collapsed = data.collapsed

  const editor = useEditor({
    editable: false,
    editorProps: {
      attributes: {
        class: 'prose prose-sm max-w-none px-3 py-2 outline-none text-fg-muted',
      },
    },
    extensions: createMinimalExtensions(),
  })

  useEffect(() => {
    if (!editor || initializedRef.current || !data.html) return
    const html = data.headerHtml ? data.headerHtml + data.html : data.html
    editor.commands.setContent(html)
    initializedRef.current = true
  }, [editor, data.html, data.headerHtml])

  return (
    <div className="border-border border-t border-l-2">
      <button
        className="text-fg-muted hover:bg-bg-secondary flex w-full cursor-pointer items-center gap-1 px-4 py-2 text-xs transition-colors"
        onClick={() => onChange({ ...data, collapsed: !collapsed })}
        type="button"
      >
        {collapsed ? (
          <ChevronRight className="h-3 w-3" />
        ) : (
          <ChevronDown className="h-3 w-3" />
        )}
        {collapsed
          ? `Show original${mode === 'forward' ? ' (forwarded)' : ''}`
          : 'Hide original'}
      </button>
      {!collapsed && (
        <div className="border-border border-l-2 opacity-50">
          <EditorContent editor={editor} />
        </div>
      )}
    </div>
  )
}
