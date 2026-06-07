// Safe-storage shim. Safari in Private Browsing throws QuotaExceededError on
// localStorage.setItem; embedded WebViews sometimes disable storage entirely;
// Notification can be undefined in some sandboxed contexts. We never want
// these environmental quirks to surface as a runtime error blanking the app
// — instead, transparently degrade to an in-memory store and a noop
// notification permission.
//
// Per-call try/catch (rather than a module-init probe) so a stubbed
// localStorage in tests, or storage permission flipping mid-session,
// transparently take effect. The fallback Map stays unused in the common
// case where storage works.

const memoryStore = new Map<string, string>()

function hasWindowStorage(): boolean {
  return typeof window !== 'undefined' && typeof window.localStorage !== 'undefined'
}

export const safeStorage = {
  getItem(key: string): null | string {
    if (hasWindowStorage()) {
      try {
        const v = window.localStorage.getItem(key)
        if (v !== null) return v
      } catch {
        // fall through to memory
      }
    }
    return memoryStore.get(key) ?? null
  },
  removeItem(key: string): void {
    if (hasWindowStorage()) {
      try {
        window.localStorage.removeItem(key)
      } catch {
        // ignore
      }
    }
    memoryStore.delete(key)
  },
  setItem(key: string, value: string): void {
    if (hasWindowStorage()) {
      try {
        window.localStorage.setItem(key, value)
        memoryStore.delete(key)
        return
      } catch {
        // quota exceeded / private mode — fall through to memory
      }
    }
    memoryStore.set(key, value)
  },
}

// Notification API access guards.
//
// `Notification` is undefined in sandboxed embeds and on Chromium when the
// site has no engagement. Even when defined, `requestPermission` can throw
// on Safari in cross-origin iframes.

export type NotificationSupport = 'denied' | 'granted' | 'unavailable' | 'unsupported'

export function getNotificationSupport(): NotificationSupport {
  if (typeof window === 'undefined' || typeof Notification === 'undefined') {
    return 'unsupported'
  }
  if (Notification.permission === 'denied') return 'denied'
  if (Notification.permission === 'granted') return 'granted'
  return 'unavailable'
}

export async function requestNotificationPermission(): Promise<
  'unsupported' | NotificationPermission
> {
  if (typeof window === 'undefined' || typeof Notification === 'undefined') {
    return 'unsupported'
  }
  try {
    return await Notification.requestPermission()
  } catch {
    return 'denied'
  }
}
