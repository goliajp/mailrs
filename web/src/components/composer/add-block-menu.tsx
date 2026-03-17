import { useState, useRef, useEffect } from 'react'
import { Plus, Type, Code2, Minus, CheckSquare, Paperclip } from 'lucide-react'
import type { BlockType } from './types'

const BLOCK_OPTIONS: { type: BlockType; label: string; icon: typeof Type; description: string }[] = [
  { type: 'text', label: 'Text', icon: Type, description: 'Rich text or markdown' },
  { type: 'code', label: 'Code', icon: Code2, description: 'Syntax-highlighted code' },
  { type: 'task', label: 'Tasks', icon: CheckSquare, description: 'Checklist' },
  { type: 'divider', label: 'Divider', icon: Minus, description: 'Visual separator' },
]

type Props = {
  onAdd: (type: BlockType) => void
  onAddFile: () => void
}

export function AddBlockMenu({ onAdd, onAddFile }: Props) {
  const [open, setOpen] = useState(false)
  const menuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const handleClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', handleClick)
    return () => document.removeEventListener('mousedown', handleClick)
  }, [open])

  return (
    <div ref={menuRef} className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-1 rounded-md px-2 py-1 text-xs text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text-secondary)]"
      >
        <Plus className="h-3.5 w-3.5" /> Add block
      </button>
      {open && (
        <div className="absolute bottom-full left-0 z-50 mb-1 w-48 rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-overlay)] py-1 shadow-lg">
          {BLOCK_OPTIONS.map(({ type, label, icon: Icon, description }) => (
            <button
              key={type}
              onClick={() => { onAdd(type); setOpen(false) }}
              className="flex w-full items-center gap-2 px-3 py-1.5 text-left transition-colors hover:bg-[var(--color-hover)]"
            >
              <Icon className="h-3.5 w-3.5 shrink-0 text-[var(--color-text-tertiary)]" />
              <div>
                <p className="text-xs font-medium text-[var(--color-text-primary)]">{label}</p>
                <p className="text-xs text-[var(--color-text-tertiary)]">{description}</p>
              </div>
            </button>
          ))}
          <hr className="my-1 border-[var(--color-border-default)]" />
          <button
            onClick={() => { onAddFile(); setOpen(false) }}
            className="flex w-full items-center gap-2 px-3 py-1.5 text-left transition-colors hover:bg-[var(--color-hover)]"
          >
            <Paperclip className="h-3.5 w-3.5 shrink-0 text-[var(--color-text-tertiary)]" />
            <div>
              <p className="text-xs font-medium text-[var(--color-text-primary)]">Attachment</p>
              <p className="text-xs text-[var(--color-text-tertiary)]">File or image</p>
            </div>
          </button>
        </div>
      )}
    </div>
  )
}
