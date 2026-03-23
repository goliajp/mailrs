import { File as FileIcon, X } from 'lucide-react'
import { useEffect, useMemo, useRef } from 'react'
import { formatFileSize } from '@/lib/html-utils'
import type { AttachmentBlockData } from '../types'

type Props = {
  data: AttachmentBlockData
  onRemove: () => void
}

export function AttachmentBlock({ data, onRemove }: Props) {
  const isImage = data.mimeType.startsWith('image/')

  const previewUrl = useMemo(
    () => (isImage ? URL.createObjectURL(data.file) : null),
    [data.file, isImage],
  )
  const prevUrlRef = useRef(previewUrl)
  useEffect(() => {
    const prev = prevUrlRef.current
    prevUrlRef.current = previewUrl
    if (prev && prev !== previewUrl) URL.revokeObjectURL(prev)
    return () => {
      if (previewUrl) URL.revokeObjectURL(previewUrl)
    }
  }, [previewUrl])

  return (
    <div className="flex items-center gap-3 rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] px-3 py-2">
      {isImage && previewUrl ? (
        <img src={previewUrl} alt={data.name} className="h-10 w-10 shrink-0 rounded object-cover" />
      ) : (
        <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded bg-[var(--color-bg-sunken)]">
          <FileIcon className="h-5 w-5 text-[var(--color-text-tertiary)]" />
        </div>
      )}
      <div className="min-w-0 flex-1">
        <p className="truncate text-sm text-[var(--color-text-primary)]" title={data.name}>
          {data.name}
        </p>
        <p className="text-xs text-[var(--color-text-tertiary)]">{formatFileSize(data.size)}</p>
      </div>
      <button
        onClick={onRemove}
        className="shrink-0 rounded-full p-1 text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text-secondary)]"
        aria-label={`Remove ${data.name}`}
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  )
}
