import type { ComposeRedraftSource } from '@/store/ui'

import { Paperclip, X } from 'lucide-react'

type Carried = ComposeRedraftSource['attachments'][number]

/**
 * The attachments a re-edit carries from the send it repairs.
 *
 * Shown as chips because there is nothing to upload — the bytes stayed on
 * the server and go out again by index. Each can be removed; removing all
 * of them is different from carrying all of them, and the send path keeps
 * that difference (`[]` versus absent).
 */
export function CarriedAttachments({
  items,
  kept,
  onToggle,
}: {
  items: readonly Carried[]
  kept: ReadonlySet<number>
  onToggle: (index: number) => void
}) {
  if (items.length === 0) return null

  return (
    <div className="border-border/60 flex flex-wrap items-center gap-1.5 border-t px-3 py-2">
      <span className="text-fg-muted text-mini mr-1 inline-flex items-center gap-1">
        <Paperclip aria-hidden className="h-3 w-3" />
        From the original
      </span>
      {items.map((item) => (
        <Chip
          item={item}
          kept={kept.has(item.index)}
          key={item.index}
          onToggle={() => onToggle(item.index)}
        />
      ))}
    </div>
  )
}

function Chip({ item, kept, onToggle }: { item: Carried; kept: boolean; onToggle: () => void }) {
  return (
    <span className={chipClass(kept)}>
      <span className="max-w-[16rem] truncate">{item.filename}</span>
      <span className="text-fg-muted text-mini">{formatSize(item.size)}</span>
      <button
        aria-label={toggleLabel(kept, item.filename)}
        className="hover:text-fg -mr-0.5 rounded p-0.5 transition-colors"
        onClick={onToggle}
        type="button"
      >
        <X aria-hidden className="h-3 w-3" />
      </button>
    </span>
  )
}

function chipClass(kept: boolean): string {
  const base =
    'inline-flex items-center gap-1.5 rounded-md border px-2 py-0.5 text-mid transition-colors'
  if (kept) return `${base} border-border bg-bg-secondary text-fg-secondary`
  // A dropped chip stays visible and struck through rather than
  // disappearing, so removing the wrong one is undoable without
  // reopening the whole re-edit.
  return `${base} border-border/40 text-fg-muted line-through`
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

function toggleLabel(kept: boolean, filename: string): string {
  if (kept) return `Remove ${filename}`
  return `Keep ${filename}`
}
