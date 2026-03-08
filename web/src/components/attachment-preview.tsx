import { File, FileText, X } from 'lucide-react'
import { useCallback, useState } from 'react'

import { formatSize } from '@/lib/format'
import type { AttachmentInfo } from '@/lib/types'
import { getToken } from '@/store/auth'

const IMAGE_EXTENSIONS = new Set(['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg', 'bmp', 'ico'])

function isImageAttachment(att: AttachmentInfo): boolean {
  if (att.content_type.startsWith('image/')) return true
  const ext = att.filename.split('.').pop()?.toLowerCase() ?? ''
  return IMAGE_EXTENSIONS.has(ext)
}

function isPdfAttachment(att: AttachmentInfo): boolean {
  if (att.content_type === 'application/pdf') return true
  return att.filename.toLowerCase().endsWith('.pdf')
}

function attachmentUrl(uid: number, index: number): string {
  const token = getToken() ?? ''
  return `/api/mail/messages/${uid}/attachments/${index}?token=${encodeURIComponent(token)}`
}

// pdf icon
function PdfIcon({ className }: { className?: string }) {
  return <FileText className={className} />
}

// generic file icon
function FileIcon({ className }: { className?: string }) {
  return <File className={className} />
}

// lightbox modal for full-size image preview
function ImageLightbox({
  src,
  alt,
  onClose,
}: {
  src: string
  alt: string
  onClose: () => void
}) {
  const handleBackdropClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) onClose()
    },
    [onClose],
  )

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    },
    [onClose],
  )

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm"
      onClick={handleBackdropClick}
      onKeyDown={handleKeyDown}
      role="dialog"
      aria-modal="true"
      aria-label={`Image preview: ${alt}`}
      tabIndex={-1}
    >
      <button
        onClick={onClose}
        className="absolute right-4 top-4 rounded-full bg-black/50 p-2 text-white transition-colors hover:bg-black/70"
        aria-label="Close preview"
      >
        <X className="h-5 w-5" />
      </button>
      <img
        src={src}
        alt={alt}
        className="max-h-[90vh] max-w-[90vw] rounded-lg object-contain shadow-2xl"
      />
    </div>
  )
}

// single image thumbnail with click-to-expand
function ImageThumbnail({ uid, index, att }: { uid: number; index: number; att: AttachmentInfo }) {
  const [lightboxOpen, setLightboxOpen] = useState(false)
  const url = attachmentUrl(uid, index)

  return (
    <>
      <div className="group relative">
        <button
          onClick={() => setLightboxOpen(true)}
          className="block overflow-hidden rounded-lg border border-zinc-200 transition-shadow hover:shadow-md dark:border-zinc-700"
          title={`${att.filename} - click to enlarge`}
        >
          <img
            src={url}
            alt={att.filename}
            className="max-h-32 object-contain"
            loading="lazy"
          />
        </button>
        <p className="mt-1 truncate text-xs text-zinc-500 dark:text-zinc-400">
          {att.filename}
          <span className="ml-1 text-zinc-400 dark:text-zinc-500">
            ({formatSize(att.size)})
          </span>
        </p>
      </div>
      {lightboxOpen && (
        <ImageLightbox
          src={url}
          alt={att.filename}
          onClose={() => setLightboxOpen(false)}
        />
      )}
    </>
  )
}

// non-image file row
function FileRow({ uid, index, att }: { uid: number; index: number; att: AttachmentInfo }) {
  const url = attachmentUrl(uid, index)
  const isPdf = isPdfAttachment(att)

  return (
    <a
      href={url}
      target="_blank"
      rel="noopener noreferrer"
      className="flex items-center gap-2 rounded-md border border-zinc-200 px-3 py-2 text-sm transition-colors hover:bg-zinc-50 dark:border-zinc-700 dark:hover:bg-zinc-800"
    >
      {isPdf ? (
        <PdfIcon className="h-5 w-5 shrink-0 text-red-500 dark:text-red-400" />
      ) : (
        <FileIcon className="h-5 w-5 shrink-0 text-zinc-400" />
      )}
      <div className="min-w-0 flex-1">
        <p className="truncate text-zinc-700 dark:text-zinc-300">
          {att.filename}
        </p>
        <p className="text-xs text-zinc-400">
          {att.content_type} · {formatSize(att.size)}
        </p>
      </div>
    </a>
  )
}

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
    <div className="border-t border-zinc-200 px-6 py-4 dark:border-zinc-800">
      <div className="mb-2 flex items-center gap-2">
        <span className="text-xs font-medium uppercase tracking-wide text-zinc-400">
          Attachments ({attachments.length})
        </span>
        <div className="h-px flex-1 bg-zinc-200 dark:bg-zinc-700" />
      </div>

      {/* image thumbnails grid */}
      {images.length > 0 && (
        <div className="mb-3 flex flex-wrap gap-3">
          {images.map(({ att, index }) => (
            <ImageThumbnail key={index} uid={uid} index={index} att={att} />
          ))}
        </div>
      )}

      {/* non-image file list */}
      {others.length > 0 && (
        <div className="flex flex-col gap-1.5">
          {others.map(({ att, index }) => (
            <FileRow key={index} uid={uid} index={index} att={att} />
          ))}
        </div>
      )}
    </div>
  )
}
