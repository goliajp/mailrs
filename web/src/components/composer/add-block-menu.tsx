import type { BlockType } from './types'

import { Minus, Paperclip, Plus, Type } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

const BLOCK_OPTIONS: { icon: typeof Type; label: string; type: BlockType }[] = [
  { icon: Type, label: 'Text', type: 'text' },
  { icon: Minus, label: 'Divider', type: 'divider' },
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
      if (menuRef.current && !menuRef.current.contains(e.target as Node))
        setOpen(false)
    }
    document.addEventListener('mousedown', handleClick)
    return () => document.removeEventListener('mousedown', handleClick)
  }, [open])

  return (
    <div className="relative" ref={menuRef}>
      <button
        className="flex items-center gap-1 rounded-md px-2 py-1 text-xs text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text-secondary)]"
        onClick={() => setOpen((v) => !v)}
        type="button"
      >
        <Plus className="h-3.5 w-3.5" /> Add block
      </button>
      {open && (
        <div className="absolute bottom-full left-0 z-50 mb-1 w-40 rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-overlay)] py-1 shadow-lg">
          {BLOCK_OPTIONS.map(({ icon: Icon, label, type }) => (
            <button
              className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)]"
              key={type}
              onClick={() => {
                onAdd(type)
                setOpen(false)
              }}
            >
              <Icon className="h-3.5 w-3.5 shrink-0 text-[var(--color-text-tertiary)]" />
              {label}
            </button>
          ))}
          <button
            className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)]"
            onClick={() => {
              onAddFile()
              setOpen(false)
            }}
          >
            <Paperclip className="h-3.5 w-3.5 shrink-0 text-[var(--color-text-tertiary)]" />
            Attachment
          </button>
        </div>
      )}
    </div>
  )
}
