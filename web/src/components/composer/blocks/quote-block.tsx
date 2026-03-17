import { ChevronDown, ChevronRight } from 'lucide-react'
import { useEditor, EditorContent } from '@tiptap/react'
import { useEffect, useRef } from 'react'
import { createMinimalExtensions } from '@/components/rich-editor'
import type { QuoteBlockData } from '../types'

type Props = {
  data: QuoteBlockData
  onChange: (data: QuoteBlockData) => void
  mode?: 'reply' | 'forward'
}

export function QuoteBlock({ data, onChange, mode }: Props) {
  const initializedRef = useRef(false)
  const collapsed = data.collapsed

  const editor = useEditor({
    extensions: createMinimalExtensions(),
    editorProps: {
      attributes: { class: 'prose prose-sm max-w-none px-3 py-2 outline-none text-[var(--color-text-tertiary)]' },
    },
    editable: false,
  })

  useEffect(() => {
    if (!editor || initializedRef.current || !data.html) return
    const html = data.headerHtml ? data.headerHtml + data.html : data.html
    editor.commands.setContent(html)
    initializedRef.current = true
  }, [editor, data.html, data.headerHtml])

  return (
    <div className="border-t border-[var(--color-border-default)]">
      <button
        type="button"
        onClick={() => onChange({ ...data, collapsed: !collapsed })}
        className="flex w-full cursor-pointer items-center gap-1 px-3 py-1.5 text-xs text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)]"
      >
        {collapsed ? <ChevronRight className="h-3 w-3" /> : <ChevronDown className="h-3 w-3" />}
        {collapsed ? `Show original${mode === 'forward' ? ' (forwarded)' : ''}` : 'Hide original'}
      </button>
      {!collapsed && (
        <div className="border-l-2 border-[var(--color-border-default)] opacity-50">
          <EditorContent editor={editor} />
        </div>
      )}
    </div>
  )
}
