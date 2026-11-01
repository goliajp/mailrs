import { toast } from '@goliapkg/gds'
import { useAtomValue } from 'jotai'
import { Bell } from 'lucide-react'
import { useCallback, useRef, useState } from 'react'

import { ActionSheet, ContextMenu, type ContextMenuItem } from '@/components/context-menu'
import { HdrBtn } from '@/components/thread-view-bubble'
import { useT } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { senderOf } from '@/lib/sender-correction'
import { authAtom } from '@/store/auth'
import { wireNotifyThread, wireSetImportant } from '@/wire/endpoints/notify'

type Position = { x: number; y: number }

/**
 * Push choices for this conversation and its sender: follow or mute the
 * conversation, go back to the settings, mark the sender as an important
 * contact.
 */
export function PushMenu({ threadId }: { threadId: string }) {
  const t = useT()
  const self = useAtomValue(authAtom)?.address ?? ''
  const [position, setPosition] = useState<null | Position>(null)
  const [sheet, setSheet] = useState(false)
  const anchor = useRef<HTMLSpanElement>(null)
  // read when the menu opens: the list cache may not hold the row yet at render
  const [sender, setSender] = useState<null | string>(null)
  const close = useCallback(() => {
    setPosition(null)
    setSheet(false)
  }, [])

  const run = (work: () => Promise<void>) => () => {
    void work()
      .then(() => toast.success(t('thread.push.done')))
      .catch((e: unknown) => toast.error(errorText(e, t('action.failed'))))
  }

  const items: ContextMenuItem[] = [
    { label: t('thread.push.follow'), onClick: run(() => wireNotifyThread(threadId, 'follow')) },
    { label: t('thread.push.mute'), onClick: run(() => wireNotifyThread(threadId, 'mute')) },
    { label: t('thread.push.reset'), onClick: run(() => wireNotifyThread(threadId, null)) },
  ]
  if (sender) {
    items.push(
      { label: t('sender.important.add'), onClick: run(() => wireSetImportant(sender, true)) },
      { label: t('sender.important.remove'), onClick: run(() => wireSetImportant(sender, false)) }
    )
  }

  const open = () => {
    setSender(senderOf(threadId, self))
    // the menu below the button on a wide screen, a bottom sheet on a phone
    if (!window.matchMedia('(min-width: 768px)').matches) {
      setSheet(true)
      return
    }
    const r = anchor.current?.getBoundingClientRect()
    if (!r) return
    setPosition({ x: Math.max(8, r.right - 220), y: r.bottom + 4 })
  }

  return (
    <>
      <span className="inline-flex" ref={anchor}>
        <HdrBtn onClick={open} title={t('thread.push.menu')}>
          <Bell className="h-4 w-4" />
        </HdrBtn>
      </span>
      <ContextMenu items={items} onClose={close} position={position} />
      <ActionSheet items={items} onClose={close} open={sheet} />
    </>
  )
}
