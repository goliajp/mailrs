import type { AttachmentInfo } from '@/lib/types'

import { File, FileText, X } from 'lucide-react'
import { useCallback, useState } from 'react'

import { Copyable } from '@/components/copy-button'
import { formatSize } from '@/lib/format'
import { getToken } from '@/store/auth'

const IMAGE_EXTENSIONS = new Set([
  'bmp',
  'gif',
  'ico',
  'jpeg',
  'jpg',
  'png',
  'svg',
  'webp',
])

export function AttachmentPreview({
  attachments,
  uid,
}: {
  attachments: AttachmentInfo[]
  uid: number
}) {
  if (attachments.length === 0) return null

  const images = attachments
    .map((att, i) => ({ att, index: i }))
    .filter(({ att }) => isImageAttachment(att))
  const others = attachments
    .map((att, i) => ({ att, index: i }))
    .filter(({ att }) => !isImageAttachment(att))

  return (
    <div className="border-border border-t px-4 py-3">
      <div className="mb-2 flex items-center gap-2">
        <span className="text-fg-muted text-xs font-medium tracking-wide uppercase select-none">
          Attachments ({attachments.length})
        </span>
        <div className="bg-border h-px flex-1" />
      </div>

      {/* image thumbnails grid */}
      {images.length > 0 && (
        <div className="mb-3 flex flex-wrap gap-2">
          {images.map(({ att, index }) => (
            <ImageThumbnail att={att} index={index} key={index} uid={uid} />
          ))}
        </div>
      )}

      {/* non-image file list */}
      {others.length > 0 && (
        <div className="flex flex-col gap-2">
          {others.map(({ att, index }) => (
            <FileRow att={att} index={index} key={index} uid={uid} />
          ))}
        </div>
      )}
    </div>
  )
}

function attachmentUrl(uid: number, index: number): string {
  const token = getToken() ?? ''
  return `/api/mail/messages/${uid}/attachments/${index}?token=${encodeURIComponent(token)}`
}

// generic file icon
function FileIcon({ className }: { className?: string }) {
  return <File className={className} />
}

// non-image file row
function FileRow({
  att,
  index,
  uid,
}: {
  att: AttachmentInfo
  index: number
  uid: number
}) {
  const url = attachmentUrl(uid, index)
  const isPdf = isPdfAttachment(att)

  return (
    <a
      className="border-border hover:bg-bg-secondary flex items-center gap-2 rounded-md border px-3 py-2 text-sm transition-colors"
      href={url}
      rel="noopener noreferrer"
      target="_blank"
    >
      {isPdf ? (
        <PdfIcon className="text-danger h-5 w-5 shrink-0" />
      ) : (
        <FileIcon className="text-fg-muted h-5 w-5 shrink-0" />
      )}
      <div className="min-w-0 flex-1">
        <p className="text-fg-secondary truncate">
          <Copyable value={att.filename}>{att.filename}</Copyable>
        </p>
        <p className="text-fg-muted text-xs">
          {att.content_type} · {formatSize(att.size)}
        </p>
      </div>
    </a>
  )
}

// lightbox modal for full-size image preview
function ImageLightbox({
  alt,
  onClose,
  src,
}: {
  alt: string
  onClose: () => void
  src: string
}) {
  const handleBackdropClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) onClose()
    },
    [onClose]
  )

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    },
    [onClose]
  )

  return (
    <div
      aria-label={`Image preview: ${alt}`}
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm"
      onClick={handleBackdropClick}
      onKeyDown={handleKeyDown}
      role="dialog"
      tabIndex={-1}
    >
      <button
        aria-label="Close preview"
        className="absolute top-4 right-4 rounded-md bg-black/50 p-2 text-white transition-colors hover:bg-black/70"
        onClick={onClose}
      >
        <X className="h-5 w-5" />
      </button>
      <img
        alt={alt}
        className="max-h-[90vh] max-w-[90vw] rounded-md object-contain shadow-lg"
        src={src}
      />
    </div>
  )
}

// single image thumbnail with click-to-expand
function ImageThumbnail({
  att,
  index,
  uid,
}: {
  att: AttachmentInfo
  index: number
  uid: number
}) {
  const [lightboxOpen, setLightboxOpen] = useState(false)
  const url = attachmentUrl(uid, index)

  return (
    <>
      <div className="group relative overflow-hidden">
        <button
          className="border-border block overflow-hidden rounded-md border transition-shadow hover:shadow-md"
          onClick={() => setLightboxOpen(true)}
          title={`${att.filename} - click to enlarge`}
        >
          <img
            alt={att.filename}
            className="max-h-32 object-contain"
            loading="lazy"
            src={url}
          />
        </button>
        <p className="text-fg-muted mt-1 truncate text-xs">
          <Copyable value={att.filename}>{att.filename}</Copyable>
          <span className="text-fg-muted ml-1">({formatSize(att.size)})</span>
        </p>
      </div>
      {lightboxOpen && (
        <ImageLightbox
          alt={att.filename}
          onClose={() => setLightboxOpen(false)}
          src={url}
        />
      )}
    </>
  )
}

function isImageAttachment(att: AttachmentInfo): boolean {
  if (att.content_type.startsWith('image/')) return true
  const ext = att.filename.split('.').pop()?.toLowerCase() ?? ''
  return IMAGE_EXTENSIONS.has(ext)
}

function isPdfAttachment(att: AttachmentInfo): boolean {
  if (att.content_type === 'application/pdf') return true
  return att.filename.toLowerCase().endsWith('.pdf')
}

// pdf icon
function PdfIcon({ className }: { className?: string }) {
  return <FileText className={className} />
}
