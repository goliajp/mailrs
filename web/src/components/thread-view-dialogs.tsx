import type { ReplyMode } from '@/components/reply-box'
import type { ThreadReplyContext } from '@/components/thread-reply-box'

import { X } from 'lucide-react'

import { DeleteThreadConfirm } from '@/components/delete-thread-confirm'
import { MobileModal } from '@/components/mobile-modal'
import { ThreadReplyBox } from '@/components/thread-reply-box'

type DialogProps = {
  handleDelete: () => void
  mobileReplyOpen: boolean
  refetchThread: () => void
  replyCtx: ThreadReplyContext
  replyMode: ReplyMode
  setForwardSource: (v: null) => void
  setMobileReplyOpen: (v: boolean) => void
  setReplyMode: (v: ReplyMode) => void
  setShowDeleteConfirm: (v: boolean) => void
  showDeleteConfirm: boolean
  subject: string
}

// the two overlays: the mobile full-screen composer and the delete
// confirmation sheet.
export function ThreadViewDialogs({
  handleDelete,
  mobileReplyOpen,
  refetchThread,
  replyCtx,
  replyMode,
  setForwardSource,
  setMobileReplyOpen,
  setReplyMode,
  setShowDeleteConfirm,
  showDeleteConfirm,
  subject,
}: DialogProps) {
  return (
    <>
      {/* mobile: full-screen reply composer */}
      {mobileReplyOpen && (
        <MobileModal className="items-end md:hidden" onClose={() => setMobileReplyOpen(false)} open>
          <div
            className="bg-surface flex h-[90dvh] w-full flex-col rounded-t-2xl"
            onClick={(e) => e.stopPropagation()}
            style={{ paddingBottom: 'var(--safe-area-bottom)' }}
          >
            {/* header */}
            <div className="border-border flex shrink-0 items-center justify-between border-b px-4 py-3">
              <button
                className="text-fg-muted hover:text-fg-secondary"
                onClick={() => setMobileReplyOpen(false)}
              >
                <X className="h-5 w-5" />
              </button>
              <span className="text-fg truncate text-sm font-medium">
                {subject || '(no subject)'}
              </span>
              <div className="w-5" />
            </div>
            {/* reply box with full height */}
            <div className="min-h-0 flex-1">
              <ThreadReplyBox
                ctx={replyCtx}
                mode={replyMode}
                onModeChange={(m) => {
                  setReplyMode(m)
                  if (m !== 'forward') setForwardSource(null)
                }}
                onSent={() => {
                  setForwardSource(null)
                  refetchThread()
                }}
              />
            </div>
          </div>
        </MobileModal>
      )}

      {/* delete confirm dialog */}
      <DeleteThreadConfirm
        onCancel={() => setShowDeleteConfirm(false)}
        onConfirm={handleDelete}
        open={showDeleteConfirm}
      />
    </>
  )
}
