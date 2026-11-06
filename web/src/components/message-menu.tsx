import { toast } from '@goliapkg/gds'
import { MoreVertical } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import { useBlockSender } from '@/hooks/use-block-sender'
import { type Key, useT } from '@/i18n'
import { type FeedbackAction, recordFeedback } from '@/lib/api'
import { errorText } from '@/lib/error-text'

const FEEDBACK_ITEMS: {
  action: FeedbackAction
  danger: boolean
  icon: string
  label: Key
}[] = [
  { action: 'mark_important', danger: false, icon: '!', label: 'feedback.important' },
  { action: 'mark_vip', danger: false, icon: '★', label: 'feedback.vip' },
  { action: 'mark_spam', danger: true, icon: '⚠', label: 'feedback.spam' },
]

type Confirming = 'block' | 'mark_spam' | null

/**
 * One message's own menu: feedback on its sender, and blocking them.
 *
 * Block puts the `From:` address on the reader's blacklist and the
 * conversation in Junk (`use-block-sender.ts`). It replaced a "Block
 * sender" item that only recorded feedback, which nothing reads, so
 * blocking from here used to do nothing at all. Not offered on the
 * reader's own messages: their own replies would go to Junk with it.
 */
export function MessageMenu({
  own,
  senderEmail,
  threadId,
}: {
  own: boolean
  senderEmail: string
  threadId: string
}) {
  const t = useT()
  const [open, setOpen] = useState(false)
  const [confirming, setConfirming] = useState<Confirming>(null)
  const ref = useRef<HTMLDivElement>(null)
  const canBlock = !own && senderEmail.includes('@')
  const sender = useBlockSender(threadId, senderEmail, open && canBlock)
  const offerBlock = canBlock && sender.ready
  const address = senderEmail.trim().toLowerCase()

  useEffect(() => {
    if (!open) return
    const handle = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false)
        setConfirming(null)
      }
    }
    document.addEventListener('mousedown', handle)
    return () => document.removeEventListener('mousedown', handle)
  }, [open])

  const close = () => {
    setOpen(false)
    setConfirming(null)
  }

  const sendFeedback = async (action: FeedbackAction) => {
    close()
    if (!senderEmail) return
    try {
      const result = await recordFeedback(senderEmail, action)
      if (result.success) {
        toast.success(result.message ?? t('feedback.recorded'))
      } else {
        toast.error(result.message ?? t('common.failed'))
      }
    } catch (err) {
      toast.error(errorText(err, t('common.failed')))
    }
  }

  const confirm = () => {
    const what = confirming
    close()
    if (what === 'block') {
      void sender.block()
      return
    }
    if (what === 'mark_spam') void sendFeedback('mark_spam')
  }

  const pickFeedback = (action: FeedbackAction) => {
    if (action === 'mark_spam') {
      setConfirming(action)
      return
    }
    void sendFeedback(action)
  }

  const unblock = () => {
    close()
    void sender.unblock()
  }

  return (
    <div className="relative" ref={ref}>
      <button
        className="text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary rounded-md p-1 transition-colors"
        onClick={() => setOpen((p) => !p)}
        title={t('feedback.title')}
      >
        <MoreVertical className="h-3.5 w-3.5" />
      </button>
      {open && (
        <div className="border-border bg-surface absolute top-full right-0 z-50 mt-1 w-48 rounded-lg border py-1 shadow-lg">
          {confirming && (
            <div className="px-3 py-2">
              <ConfirmText address={address} allowed={sender.allowed} confirming={confirming} />
              <div className="mt-2 flex gap-2">
                <button
                  className="text-fg-muted hover:bg-bg-secondary rounded px-2 py-1 text-xs"
                  onClick={() => setConfirming(null)}
                >
                  {t('common.cancel')}
                </button>
                <button
                  className="bg-danger rounded px-2 py-1 text-xs text-white hover:opacity-90"
                  onClick={confirm}
                >
                  {t('common.confirm')}
                </button>
              </div>
            </div>
          )}
          {!confirming && (
            <>
              <p className="text-fg-muted md:text-mini truncate px-3 py-1 text-xs">{senderEmail}</p>
              {FEEDBACK_ITEMS.map((item) => (
                <MenuItem
                  danger={item.danger}
                  icon={item.icon}
                  key={item.action}
                  label={t(item.label)}
                  onClick={() => pickFeedback(item.action)}
                />
              ))}
              {offerBlock && sender.blocked && (
                <MenuItem
                  danger={false}
                  icon="✘"
                  label={t('block.unblockItem', { address })}
                  onClick={unblock}
                />
              )}
              {offerBlock && !sender.blocked && (
                <MenuItem
                  danger
                  icon="✘"
                  label={t('block.item', { address })}
                  onClick={() => setConfirming('block')}
                />
              )}
            </>
          )}
        </div>
      )}
    </div>
  )
}

function ConfirmText({
  address,
  allowed,
  confirming,
}: {
  address: string
  allowed: boolean
  confirming: 'block' | 'mark_spam'
}) {
  const t = useT()
  if (confirming === 'mark_spam') {
    return <p className="text-fg-secondary text-xs">{t('feedback.confirmSpam')}</p>
  }
  return (
    <>
      <p className="text-fg-secondary text-xs break-words">{t('block.confirm', { address })}</p>
      {allowed && (
        <p className="text-warning mt-1 text-xs break-words">
          {t('block.confirmAllowed', { address })}
        </p>
      )}
    </>
  )
}

function itemTone(danger: boolean): string {
  if (danger) return 'text-danger hover:bg-danger/10'
  return 'text-fg-secondary hover:bg-bg-secondary'
}

function MenuItem({
  danger,
  icon,
  label,
  onClick,
}: {
  danger: boolean
  icon: string
  label: string
  onClick: () => void
}) {
  return (
    <button
      className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs transition-colors ${itemTone(danger)}`}
      onClick={onClick}
    >
      <span className="w-4 shrink-0 text-center">{icon}</span>
      <span className="min-w-0 truncate">{label}</span>
    </button>
  )
}
