import { useState } from 'react'

/// A PDF attachment shown in place, on request. Not sandboxed: Chrome
/// shows no PDF in a sandboxed frame, and the server serves the part as
/// `application/pdf` with `nosniff`, framed only by this site. Callers
/// offer it only for a part declared `application/pdf`; a name ending in
/// `.pdf` is the sender's word and would be served as whatever it says.
export function PdfPreview({ filename, url }: { filename: string; url: string }) {
  const [open, setOpen] = useState(false)
  return (
    <>
      <button
        aria-expanded={open}
        className="text-fg-muted hover:bg-bg-secondary shrink-0 rounded-md px-2 py-0.5 text-xs transition-colors"
        onClick={() => setOpen((v) => !v)}
        type="button"
      >
        {open ? 'Hide preview' : 'Preview'}
      </button>
      {open && (
        <iframe
          className="border-border h-96 w-full basis-full border-t"
          src={url}
          title={filename}
        />
      )}
    </>
  )
}
