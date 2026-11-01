import type { WireAttachmentPreview } from '@/wire/schemas/attachment-preview'

import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { either, useT } from '@/i18n'
import { wireAttachmentPreview } from '@/wire/endpoints/attachment-preview'

/// A Word, Excel or PowerPoint attachment's text, shown in place on request.
/// The server reads the file; nothing the sender wrote runs here.
export function OfficePreview({ index, uid }: { index: number; uid: number }) {
  const t = useT()
  const [open, setOpen] = useState(false)
  return (
    <>
      <button
        aria-expanded={open}
        className="text-fg-muted hover:bg-bg-secondary shrink-0 rounded-md px-2 py-0.5 text-xs transition-colors"
        onClick={() => setOpen((v) => !v)}
        type="button"
      >
        {t(either(open, 'pdf.hidePreview', 'compose.preview'))}
      </button>
      {open && <OfficeText index={index} uid={uid} />}
    </>
  )
}

function OfficeText({ index, uid }: { index: number; uid: number }) {
  const t = useT()
  const { data, isError, isLoading } = useQuery({
    queryKey: ['attachment-preview', uid, index],
    staleTime: 5 * 60_000,
    queryFn: ({ signal }) => wireAttachmentPreview(uid, index, signal),
  })
  return (
    <div className="border-border max-h-96 w-full basis-full overflow-auto border-t pt-2 text-sm">
      {isLoading && <p className="text-fg-muted text-xs">{t('common.loading')}</p>}
      {(isError || data?.success === false) && (
        <p className="text-fg-muted text-xs">{t('office.unreadable')}</p>
      )}
      {data?.success && <Sections preview={data} />}
    </div>
  )
}

/// A sheet's rows carry tabs between cells and read as a table; a document's
/// or a slide's lines read as paragraphs.
function SectionBody({ lines }: { lines: string[] }) {
  if (lines.some((l) => l.includes('\t'))) {
    return (
      <table className="text-xs">
        <tbody>
          {lines.map((l, r) => (
            <tr className="border-border border-b" key={r}>
              {l.split('\t').map((cell, c) => (
                <td className="px-2 py-0.5 whitespace-nowrap" key={c}>
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    )
  }
  return (
    <div className="text-fg-secondary space-y-1 whitespace-pre-wrap">
      {lines.map((l, i) => (
        <p key={i}>{l}</p>
      ))}
    </div>
  )
}

function Sections({ preview }: { preview: WireAttachmentPreview }) {
  const t = useT()
  return (
    <div className="space-y-3">
      {preview.sections.map((s, i) => (
        <section key={i}>
          {s.title && <h4 className="text-fg-muted mb-1 text-xs font-medium">{s.title}</h4>}
          <SectionBody lines={s.lines} />
        </section>
      ))}
      {preview.truncated && <p className="text-fg-muted text-xs">{t('office.truncated')}</p>}
    </div>
  )
}
