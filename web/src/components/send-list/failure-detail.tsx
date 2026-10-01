import type { WireSend } from '@/wire/schemas/sends'

import { Pencil, RotateCw, Trash2 } from 'lucide-react'

import { failedRecipients } from './send-model'

/**
 * Why a send did not arrive, and what to do about it: send it again,
 * edit it, or delete it once it no longer matters.
 *
 * Shown on expand for a failed or partly-delivered send. The remote's
 * reply is quoted verbatim — a paraphrase of `550 5.1.1 no such user`
 * loses the one detail that says whether to fix the address or wait.
 */
export function FailureDetail({
  deleting,
  onDelete,
  onRedraft,
  onResend,
  resending,
  send,
}: {
  deleting: boolean
  onDelete: () => void
  onRedraft: () => void
  onResend: () => void
  resending: boolean
  send: WireSend
}) {
  const failed = failedRecipients(send)

  return (
    <div className="border-border/60 bg-bg-secondary/40 border-t px-4 py-3">
      {failed.length > 0 && (
        <ul className="mb-3 space-y-1.5">
          {failed.map((r) => (
            <li className="text-mid flex flex-col gap-0.5" key={r.recipient}>
              <span className="text-fg-secondary font-medium">{r.recipient}</span>
              <span className="text-fg-muted text-mini font-mono">
                {replyLine(r.code, r.message)}
              </span>
            </li>
          ))}
        </ul>
      )}

      <div className="flex flex-wrap items-center gap-2">
        {send.can_resend && (
          <>
            <button
              className="border-border hover:bg-bg-secondary text-fg-secondary inline-flex items-center gap-1.5 rounded-md border px-2.5 py-1 text-xs transition-colors disabled:opacity-50"
              disabled={resending}
              onClick={onResend}
              type="button"
            >
              <RotateCw aria-hidden className="h-3.5 w-3.5" />
              {resendLabel(resending)}
            </button>
            <button
              className="border-border hover:bg-bg-secondary text-fg-secondary inline-flex items-center gap-1.5 rounded-md border px-2.5 py-1 text-xs transition-colors"
              onClick={onRedraft}
              type="button"
            >
              <Pencil aria-hidden className="h-3.5 w-3.5" />
              Edit and send again
            </button>
          </>
        )}
        <button
          className="border-border hover:bg-bg-secondary text-fg-secondary hover:text-danger inline-flex items-center gap-1.5 rounded-md border px-2.5 py-1 text-xs transition-colors disabled:opacity-50"
          disabled={deleting}
          onClick={onDelete}
          type="button"
        >
          <Trash2 aria-hidden className="h-3.5 w-3.5" />
          {deleteLabel(deleting)}
        </button>
      </div>

      {!send.can_resend && (
        <p className="text-fg-muted text-mini mt-2">
          The stored copy of this message is missing, so it cannot be resent or edited. Its text is
          still in the conversation.
        </p>
      )}
    </div>
  )
}

function deleteLabel(deleting: boolean): string {
  if (deleting) return 'Deleting…'
  return 'Delete'
}

/** `550 5.1.1 no such user`, or just the code when the remote said nothing. */
function replyLine(code: number, message: string): string {
  const parts: string[] = []
  if (code > 0) parts.push(String(code))
  if (message.trim()) parts.push(message.trim())
  if (parts.length === 0) return 'No reply recorded'
  return parts.join(' ')
}

function resendLabel(resending: boolean): string {
  if (resending) return 'Resending…'
  return 'Resend'
}
