import { toast } from '@goliapkg/gds'
import { MoreVertical } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import { type Key, useT } from '@/i18n'
import { type FeedbackAction, recordFeedback } from '@/lib/api'
import { errorText } from '@/lib/error-text'

/**
 * A button in the reading pane's header bar.
 *
 * `disabled` is a real prop now. The prev/next arrows faked it with
 * `pointer-events-none opacity-30`, which blocks the mouse and not the
 * keyboard — tab to the greyed arrow, press Enter, and it moved. It
 * also announced as an ordinary enabled button, and `opacity-30` on
 * `text-fg-muted` is below any readable contrast.
 */
export function HdrBtn({
  children,
  className,
  disabled,
  onClick,
  title,
}: {
  children: React.ReactNode
  className?: string
  disabled?: boolean
  onClick: () => void
  title: string
}) {
  return (
    <button
      className={`text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary focus-visible:ring-accent/50 flex h-7 w-7 items-center justify-center rounded-md transition-colors focus-visible:ring-2 focus-visible:outline-none disabled:pointer-events-none disabled:opacity-40 ${className ?? ''}`}
      disabled={disabled}
      onClick={onClick}
      title={title}
      type="button"
    >
      {children}
    </button>
  )
}

export function SmBtn({
  children,
  onClick,
  title,
}: {
  children: React.ReactNode
  onClick: () => void
  title: string
}) {
  return (
    <button
      className="text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary flex h-7 w-7 items-center justify-center rounded-md transition-all duration-150"
      onClick={onClick}
      title={title}
    >
      {children}
    </button>
  )
}

function confirmLabel(action: FeedbackAction): Key {
  if (action === 'block') return 'feedback.confirmBlock'
  return 'feedback.confirmSpam'
}

const FEEDBACK_ITEMS: {
  action: FeedbackAction
  icon: string
  label: Key
}[] = [
  { action: 'mark_important', icon: '!', label: 'feedback.important' },
  { action: 'mark_vip', icon: '\u2605', label: 'feedback.vip' },
  { action: 'mark_spam', icon: '\u26A0', label: 'feedback.spam' },
  { action: 'block', icon: '\u2718', label: 'feedback.block' },
]

export function FeedbackMenu({ senderEmail }: { senderEmail: string }) {
  const t = useT()
  const [open, setOpen] = useState(false)
  const [confirming, setConfirming] = useState<FeedbackAction | null>(null)
  const ref = useRef<HTMLDivElement>(null)

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

  const executeAction = async (action: FeedbackAction) => {
    setOpen(false)
    setConfirming(null)
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

  const handleAction = (action: FeedbackAction) => {
    if (action === 'block' || action === 'mark_spam') {
      setConfirming(action)
    } else {
      executeAction(action)
    }
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
          {confirming ? (
            <div className="px-3 py-2">
              <p className="text-fg-secondary text-xs">{t(confirmLabel(confirming))}</p>
              <div className="mt-2 flex gap-2">
                <button
                  className="text-fg-muted hover:bg-bg-secondary rounded px-2 py-1 text-xs"
                  onClick={() => setConfirming(null)}
                >
                  {t('common.cancel')}
                </button>
                <button
                  className="bg-danger rounded px-2 py-1 text-xs text-white hover:opacity-90"
                  onClick={() => executeAction(confirming)}
                >
                  {t('common.confirm')}
                </button>
              </div>
            </div>
          ) : (
            <>
              <p className="text-fg-muted md:text-mini truncate px-3 py-1 text-xs">{senderEmail}</p>
              {FEEDBACK_ITEMS.map((item) => (
                <button
                  className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs transition-colors ${
                    item.action === 'block' || item.action === 'mark_spam'
                      ? 'text-danger hover:bg-danger/10'
                      : 'text-fg-secondary hover:bg-bg-secondary'
                  }`}
                  key={item.action}
                  onClick={() => handleAction(item.action)}
                >
                  <span className="w-4 text-center">{item.icon}</span>
                  {t(item.label)}
                </button>
              ))}
            </>
          )}
        </div>
      )}
    </div>
  )
}
