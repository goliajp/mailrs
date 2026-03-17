import { useEffect, useRef } from 'react'
import { EditorView, keymap, placeholder as cmPlaceholder } from '@codemirror/view'
import { EditorState } from '@codemirror/state'
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands'
import { oneDark } from '@codemirror/theme-one-dark'
import { javascript } from '@codemirror/lang-javascript'
import type { CodeBlockData } from '../types'

const LANGUAGES = ['javascript', 'typescript', 'python', 'rust', 'go', 'java', 'html', 'css', 'sql', 'bash', 'json', 'yaml', 'markdown', 'plaintext']

const cmTheme = EditorView.theme({
  '&': { fontSize: '13px', borderRadius: '0 0 8px 8px' },
  '.cm-content': { fontFamily: '"SF Mono", Monaco, Consolas, monospace', padding: '8px 12px' },
  '.cm-gutters': { display: 'none' },
  '.cm-focused': { outline: 'none' },
  '.cm-placeholder': { color: 'var(--color-text-tertiary)' },
})

type Props = {
  data: CodeBlockData
  onChange: (data: CodeBlockData) => void
  disabled?: boolean
}

export function CodeBlock({ data, onChange, disabled }: Props) {
  const containerRef = useRef<HTMLDivElement>(null)
  const viewRef = useRef<EditorView | null>(null)

  useEffect(() => {
    if (!containerRef.current || viewRef.current) return

    const view = new EditorView({
      state: EditorState.create({
        doc: data.code,
        extensions: [
          javascript(), // default; could switch based on language
          history(),
          keymap.of([...defaultKeymap, ...historyKeymap]),
          cmPlaceholder('Paste or type code...'),
          oneDark,
          cmTheme,
          EditorView.lineWrapping,
          EditorView.updateListener.of((update) => {
            if (update.docChanged) {
              onChange({ ...data, code: update.state.doc.toString() })
            }
          }),
          EditorView.editable.of(!disabled),
        ],
      }),
      parent: containerRef.current,
    })
    viewRef.current = view
    return () => { view.destroy(); viewRef.current = null }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  return (
    <div className="overflow-hidden rounded-lg border border-[var(--color-border-default)]">
      <div className="flex items-center gap-2 border-b border-[var(--color-border-default)] bg-[var(--color-bg-raised)] px-4 py-1.5">
        <span className="text-[10px] font-medium text-[var(--color-text-tertiary)]">CODE</span>
        <select
          value={data.language}
          onChange={(e) => onChange({ ...data, language: e.target.value })}
          className="rounded bg-transparent px-1 py-0.5 text-xs text-[var(--color-text-secondary)] outline-none"
        >
          {LANGUAGES.map((l) => (
            <option key={l} value={l}>{l}</option>
          ))}
        </select>
      </div>
      <div ref={containerRef} />
    </div>
  )
}
