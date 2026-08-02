// The action vocabularies the conversation list speaks — their own module
// so the list, the row and the batch bar can each name them without one
// importing the other for a type alone.

export type BatchAction = 'archive' | 'delete' | 'read' | 'star' | 'unarchive' | 'unread' | 'unstar'
// v2.4.1 Phase 3 (RFC-B §3.4/§3.8) — single-thread junk moves. Kept
// out of BatchAction because the batch mutation endpoint doesn't
// support them yet; adding them there would need a matching backend
// batch handler.
// v2.9 triage — bucket moves between Inbox / Notifications / Promotions
// / Junk. Kept out of BatchAction (no matching backend batch handler).
export type JunkAction =
  | 'mark-junk'
  | 'mark-not-junk'
  | 'mark-notification'
  | 'mark-promotion'
  | 'move-to-inbox'
export type SingleAction = 'pin' | 'snooze' | 'unpin' | BatchAction | JunkAction
