/**
 * Words for screen readers about something that changed out of sight —
 * new mail arriving — read politely, without moving focus. One region for
 * the app, rendered by `LiveAnnouncer`.
 */

type Listener = (text: string) => void

let listener: Listener | null = null

export function announce(text: string) {
  listener?.(text)
}

export function setAnnouncer(l: Listener | null) {
  listener = l
}
