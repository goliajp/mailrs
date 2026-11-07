import type { WireDraftAttachment } from '@/wire/schemas/mail'

import { useCallback, useMemo, useState } from 'react'

import { errorText } from '@/lib/error-text'
import { wireUploadDraftAttachment } from '@/wire/endpoints/draft-attachments'

/**
 * A file in the composer. It is uploaded the moment it is added; once the
 * server has it the draft names it (`stored`). One the server refused stays
 * here as a file (`refused`) and goes in the send's form, as one still
 * `uploading` does when the send comes first.
 */
export type DraftFile =
  | { file: File; key: string; kind: 'refused'; reason: string }
  | { file: File; key: string; kind: 'uploading' }
  | { key: string; kind: 'stored'; stored: WireDraftAttachment }

export type DraftFiles = ReturnType<typeof useDraftFiles>

/** What a draft's files are, for telling whether a save carried them. */
export function filesKey(stored: readonly WireDraftAttachment[]): string {
  return stored.map((a) => a.blob_id).join(',')
}

export function useDraftFiles(initial: readonly WireDraftAttachment[]) {
  const [files, setFiles] = useState<DraftFile[]>(() => initial.map(storedFile))

  const settle = useCallback((key: string, next: DraftFile) => {
    // a file removed while it uploaded stays removed
    setFiles((prev) => prev.map((f) => replaceIf(f, key, next)))
  }, [])

  const add = useCallback(
    (added: readonly File[]) => {
      for (const file of added) {
        const key = crypto.randomUUID()
        setFiles((prev) => [...prev, { file, key, kind: 'uploading' }])
        wireUploadDraftAttachment(file).then(
          (stored) => settle(key, { key, kind: 'stored', stored }),
          (err: unknown) =>
            settle(key, { file, key, kind: 'refused', reason: errorText(err, String(err)) })
        )
      }
    },
    [settle]
  )

  const remove = useCallback((key: string) => {
    setFiles((prev) => prev.filter((f) => f.key !== key))
  }, [])

  /** The files of a draft the session came back to, or none after a send. */
  const replace = useCallback((stored: readonly WireDraftAttachment[]) => {
    setFiles(stored.map(storedFile))
  }, [])

  const stored = useMemo(() => files.flatMap(storedOf), [files])
  const unsaved = useMemo(() => files.flatMap(fileOf), [files])
  const refused = useMemo(() => files.flatMap(refusalOf), [files])

  return { add, all: files, refused, remove, replace, stored, unsaved }
}

function fileOf(f: DraftFile): File[] {
  if (f.kind === 'stored') return []
  return [f.file]
}

function refusalOf(f: DraftFile): { key: string; name: string; reason: string }[] {
  if (f.kind !== 'refused') return []
  return [{ key: f.key, name: f.file.name, reason: f.reason }]
}

function replaceIf(f: DraftFile, key: string, next: DraftFile): DraftFile {
  if (f.key === key) return next
  return f
}

function storedFile(stored: WireDraftAttachment): DraftFile {
  // not the blob id: the same file can be attached twice
  return { key: crypto.randomUUID(), kind: 'stored', stored }
}

function storedOf(f: DraftFile): WireDraftAttachment[] {
  if (f.kind === 'stored') return [f.stored]
  return []
}
