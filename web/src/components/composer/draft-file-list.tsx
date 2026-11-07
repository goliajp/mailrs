import type { DraftFile, DraftFiles } from './use-draft-files'

import { Paperclip, X } from 'lucide-react'

import { useT } from '@/i18n'
import { formatFileSize } from '@/lib/html-utils'
import { draftAttachmentUrl } from '@/wire/endpoints/draft-attachments'

/**
 * The files in the composer. One the server keeps opens through the draft
 * file route; one still uploading, or refused, is only in this window.
 */
export function DraftFileList({ files }: { files: DraftFiles }) {
  if (files.all.length === 0) return null
  return (
    <ul className="border-border/60 flex shrink-0 flex-wrap gap-1.5 border-t px-3 py-2">
      {files.all.map((f) => (
        <Row file={f} key={f.key} onRemove={() => files.remove(f.key)} />
      ))}
    </ul>
  )
}

function Name({ file }: { file: DraftFile }) {
  const t = useT()
  if (file.kind === 'stored')
    return (
      <a
        className="hover:text-fg max-w-[16rem] truncate underline-offset-2 hover:underline"
        href={draftAttachmentUrl(file.stored)}
        rel="noopener"
        target="_blank"
      >
        {file.stored.name}
      </a>
    )
  return (
    <>
      <span className="max-w-[16rem] truncate">{file.file.name}</span>
      {file.kind === 'uploading' && (
        <span className="text-fg-muted text-mini">{t('composer.uploading')}</span>
      )}
    </>
  )
}

function nameOf(f: DraftFile): string {
  if (f.kind === 'stored') return f.stored.name
  return f.file.name
}

function Row({ file, onRemove }: { file: DraftFile; onRemove: () => void }) {
  const t = useT()
  const name = nameOf(file)
  return (
    <li className="border-border bg-bg-secondary text-fg-secondary text-mid inline-flex items-center gap-1.5 rounded-md border px-2 py-0.5">
      <Paperclip aria-hidden className="text-fg-muted h-3 w-3 shrink-0" />
      <Name file={file} />
      <span className="text-fg-muted text-mini">{formatFileSize(sizeOf(file))}</span>
      <button
        aria-label={t('composer.removeNamed', { name })}
        className="hover:text-fg -mr-0.5 rounded p-0.5 transition-colors"
        onClick={onRemove}
        type="button"
      >
        <X aria-hidden className="h-3 w-3" />
      </button>
    </li>
  )
}

function sizeOf(f: DraftFile): number {
  if (f.kind === 'stored') return f.stored.size
  return f.file.size
}
