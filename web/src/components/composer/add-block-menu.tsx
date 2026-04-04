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
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', handleClick)
    return () => document.removeEventListener('mousedown', handleClick)
  }, [open])

  return (
    <div className="relative" ref={menuRef}>
      <button
        className="text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary flex items-center gap-1 rounded-md px-2 py-1 text-xs transition-colors"
        onClick={() => setOpen((v) => !v)}
        type="button"
      >
        <Plus className="h-3.5 w-3.5" /> Add block
      </button>
      {open && (
        <div className="border-border bg-surface absolute bottom-full left-0 z-50 mb-1 w-40 rounded-lg border py-1 shadow-lg">
          {BLOCK_OPTIONS.map(({ icon: Icon, label, type }) => (
            <button
              className="text-fg-secondary hover:bg-bg-secondary flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs transition-colors"
              key={type}
              onClick={() => {
                onAdd(type)
                setOpen(false)
              }}
            >
              <Icon className="text-fg-muted h-3.5 w-3.5 shrink-0" />
              {label}
            </button>
          ))}
          <button
            className="text-fg-secondary hover:bg-bg-secondary flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs transition-colors"
            onClick={() => {
              onAddFile()
              setOpen(false)
            }}
          >
            <Paperclip className="text-fg-muted h-3.5 w-3.5 shrink-0" />
            Attachment
          </button>
        </div>
      )}
    </div>
  )
}
