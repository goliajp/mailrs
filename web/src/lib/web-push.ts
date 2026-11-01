import { wireWebPushSubscribe, wireWebPushUnsubscribe } from '@/wire/endpoints/web-push'

/**
 * The push worker's own scope. It never controls a page, so it can never
 * stand in for `/sw.js`, the kill switch that unsticks browsers still
 * holding the old caching worker at `/`.
 */
export const PUSH_SW_SCOPE = '/push-sw/'
const PUSH_SW_URL = '/push-sw.js'

export type EnableResult = 'denied' | 'subscribed'

/** This browser's current subscription, if it has one. */
export async function currentSubscription(): Promise<null | PushSubscription> {
  const reg = await registration()
  if (!reg) return null
  return reg.pushManager.getSubscription()
}

/** Stop pushes to this browser, on both ends. */
export async function disableWebPush(): Promise<void> {
  const sub = await currentSubscription()
  if (!sub) return
  await wireWebPushUnsubscribe(sub.endpoint)
  await sub.unsubscribe()
}

/** Ask for permission, subscribe this browser and hand the subscription to the server. */
export async function enableWebPush(publicKey: string): Promise<EnableResult> {
  const permission = await Notification.requestPermission()
  if (permission !== 'granted') return 'denied'
  const reg = await navigator.serviceWorker.register(PUSH_SW_URL, { scope: PUSH_SW_SCOPE })
  await navigator.serviceWorker.ready
  const sub = await reg.pushManager.subscribe({
    applicationServerKey: keyBytes(publicKey),
    userVisibleOnly: true,
  })
  const json = sub.toJSON()
  await wireWebPushSubscribe({
    endpoint: sub.endpoint,
    keys: { auth: json.keys?.auth ?? '', p256dh: json.keys?.p256dh ?? '' },
  })
  return 'subscribed'
}

/** Remove every service worker except the push worker. */
export async function keepOnlyPushWorker(sw: ServiceWorkerContainer): Promise<void> {
  const regs = await sw.getRegistrations()
  for (const r of regs) {
    if (!new URL(r.scope).pathname.endsWith(PUSH_SW_SCOPE)) await r.unregister()
  }
}

/** The VAPID public key as the bytes `applicationServerKey` takes. */
export function keyBytes(base64url: string): Uint8Array<ArrayBuffer> {
  const pad = '='.repeat((4 - (base64url.length % 4)) % 4)
  const raw = atob((base64url + pad).replace(/-/g, '+').replace(/_/g, '/'))
  const out = new Uint8Array(new ArrayBuffer(raw.length))
  for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i)
  return out
}

/** Whether this browser can take Web Push at all. */
export function webPushSupported(): boolean {
  return 'serviceWorker' in navigator && 'PushManager' in window && 'Notification' in window
}

async function registration(): Promise<ServiceWorkerRegistration | undefined> {
  return navigator.serviceWorker.getRegistration(PUSH_SW_SCOPE)
}
