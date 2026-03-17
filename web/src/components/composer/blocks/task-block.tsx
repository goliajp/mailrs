import { Plus, X } from 'lucide-react'
import type { TaskBlockData } from '../types'

type Props = {
  data: TaskBlockData
  onChange: (data: TaskBlockData) => void
}

export function TaskBlock({ data, onChange }: Props) {
  const toggleItem = (id: string) => {
    onChange({
      items: data.items.map((item) =>
        item.id === id ? { ...item, checked: !item.checked } : item
      ),
    })
  }

  const updateText = (id: string, text: string) => {
    onChange({
      items: data.items.map((item) =>
        item.id === id ? { ...item, text } : item
      ),
    })
  }

  const removeItem = (id: string) => {
    onChange({ items: data.items.filter((item) => item.id !== id) })
  }

  const addItem = () => {
    onChange({
      items: [...data.items, { id: crypto.randomUUID(), text: '', checked: false }],
    })
  }

  const handleKeyDown = (e: React.KeyboardEvent, id: string) => {
    if (e.key === 'Enter') {
      e.preventDefault()
      addItem()
    }
    if (e.key === 'Backspace' && data.items.find((i) => i.id === id)?.text === '') {
      e.preventDefault()
      if (data.items.length > 1) removeItem(id)
    }
  }

  return (
    <div className="rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)]">
      <div className="border-b border-[var(--color-border-default)] px-4 py-1.5">
        <span className="text-xs font-medium text-[var(--color-text-tertiary)]">TASKS</span>
      </div>
      <div className="px-3 py-1">
        {data.items.map((item) => (
          <div key={item.id} className="group flex items-center gap-2 py-0.5">
            <input
              type="checkbox"
              checked={item.checked}
              onChange={() => toggleItem(item.id)}
              className="h-3.5 w-3.5 shrink-0 rounded border-[var(--color-border-default)]"
            />
            <input
              type="text"
              value={item.text}
              onChange={(e) => updateText(item.id, e.target.value)}
              onKeyDown={(e) => handleKeyDown(e, item.id)}
              placeholder="Task..."
              className={`flex-1 bg-transparent text-sm outline-none placeholder:text-[var(--color-text-tertiary)] ${
                item.checked ? 'text-[var(--color-text-tertiary)] line-through' : 'text-[var(--color-text-primary)]'
              }`}
            />
            <button
              onClick={() => removeItem(item.id)}
              className="shrink-0 rounded p-0.5 text-[var(--color-text-tertiary)] opacity-0 transition-opacity hover:text-[var(--color-text-secondary)] group-hover:opacity-100"
            >
              <X className="h-3 w-3" />
            </button>
          </div>
        ))}
      </div>
      <button
        onClick={addItem}
        className="flex w-full items-center gap-1 border-t border-[var(--color-border-default)] px-4 py-1.5 text-xs text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)]"
      >
        <Plus className="h-3 w-3" /> Add task
      </button>
    </div>
  )
}
