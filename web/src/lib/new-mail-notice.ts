import type { NewMessageEvent } from '@/lib/types'

/// A desktop notice for mail that arrived while the tab was hidden.
/// Clicking it brings the tab forward and opens the conversation; it
/// used to do nothing but close.
export function showNewMailNotice(
  msg: Pick<NewMessageEvent, 'sender' | 'snippet' | 'subject' | 'thread_id'>,
  open: (threadId: string) => void
): void {
  // 'Notification' is absent on iOS Safari (non-PWA)
  if (!('Notification' in window) || Notification.permission !== 'granted' || !document.hidden) {
    return
  }
  const notice = new Notification(msg.sender, {
    body: msg.subject || msg.snippet,
    tag: msg.thread_id,
  })
  notice.onclick = () => {
    window.focus()
    open(msg.thread_id)
    notice.close()
  }
}
