import type { AttachmentBlockData } from '../types'

import { File as FileIcon, X } from 'lucide-react'
import { useEffect, useMemo, useRef } from 'react'

import { formatFileSize } from '@/lib/html-utils'

type Props = {
  data: AttachmentBlockData
  onRemove: () => void
}

export function AttachmentBlock({ data, onRemove }: Props) {
  const isImage = data.mimeType.startsWith('image/')

  const previewUrl = useMemo(
    () => (isImage ? URL.createObjectURL(data.file) : null),
    [data.file, isImage]
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
    <div className="border-border bg-surface flex items-center gap-3 rounded-lg border px-3 py-2">
      {isImage && previewUrl ? (
        <img alt={data.name} className="h-10 w-10 shrink-0 rounded object-cover" src={previewUrl} />
      ) : (
        <div className="bg-bg-secondary flex h-10 w-10 shrink-0 items-center justify-center rounded">
          <FileIcon className="text-fg-muted h-5 w-5" />
        </div>
      )}
      <div className="min-w-0 flex-1">
        <p className="text-fg truncate text-sm" title={data.name}>
          {data.name}
        </p>
        <p className="text-fg-muted text-xs">{formatFileSize(data.size)}</p>
      </div>
      <button
        aria-label={`Remove ${data.name}`}
        className="text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary shrink-0 rounded-full p-1 transition-colors"
        onClick={onRemove}
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  )
}
