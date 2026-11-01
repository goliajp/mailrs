import type { z } from '@/lib/zod'

import { wireFetch } from '../client'
import { emptyResponseSchema } from '../schemas/mutations'
import { webPushStateSchema } from '../schemas/notify'

/** The fields of `PushSubscription.toJSON()` the server stores. */
export type WebPushSubscriptionBody = {
  endpoint: string
  keys: { auth: string; p256dh: string }
}

/** Whether the server has Web Push configured, and the key to subscribe with. */
export function wireWebPushState(): Promise<z.infer<typeof webPushStateSchema>> {
  return wireFetch(webPushStateSchema, { path: '/push/web' })
}

export async function wireWebPushSubscribe(sub: WebPushSubscriptionBody): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { endpoint: sub.endpoint, keys: { auth: sub.keys.auth, p256dh: sub.keys.p256dh } },
    method: 'POST',
    path: '/push/web-subscriptions',
  })
}

export async function wireWebPushUnsubscribe(endpoint: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { endpoint },
    method: 'DELETE',
    path: '/push/web-subscriptions',
  })
}
