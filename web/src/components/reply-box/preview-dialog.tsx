import { useEffect } from 'react'

type PreviewDialogProps = {
  html: string
  onClose: () => void
  onSend: () => void
  sending?: boolean
}

const IFRAME_BASE =
  '<!doctype html><html><head><meta charset="utf-8"><style>body{font-family:system-ui,-apple-system,sans-serif;font-size:14px;line-height:1.5;color:#222;background:#fff;padding:16px;margin:0}*{max-width:100%}</style></head><body>'

export function PreviewDialog({ html, onClose, onSend, sending }: PreviewDialogProps) {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [onClose])

  return (
    <div
      aria-modal="true"
      className="bg-fg/40 fixed inset-0 z-50 flex items-center justify-center p-4 backdrop-blur-sm"
      onClick={onClose}
      role="dialog"
    >
      <div
        className="border-border bg-bg flex max-h-[85vh] w-full max-w-3xl flex-col rounded-lg border shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="border-border flex items-center justify-between border-b px-4 py-2">
          <div className="text-fg text-sm font-medium">Preview — receiver&apos;s view</div>
          <button className="text-fg-muted hover:text-fg text-xs" onClick={onClose} type="button">
            Close
          </button>
        </div>
        <iframe
          className="bg-bg-secondary min-h-[60vh] flex-1 rounded-b-lg"
          sandbox=""
          srcDoc={`${IFRAME_BASE}${html}</body></html>`}
          title="Message preview"
        />
        <div className="border-border flex items-center justify-end gap-2 border-t px-4 py-2">
          <button
            className="border-border text-fg hover:bg-bg-tertiary rounded-md border px-3 py-1.5 text-xs"
            onClick={onClose}
            type="button"
          >
            Back to edit
          </button>
          <button
            className="bg-accent hover:bg-accent-hover rounded-md px-3 py-1.5 text-xs font-medium text-white disabled:opacity-50"
            disabled={sending}
            onClick={onSend}
            type="button"
          >
            Send
          </button>
        </div>
      </div>
    </div>
  )
}
