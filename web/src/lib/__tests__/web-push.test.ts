import { afterEach, describe, expect, it, vi } from 'vitest'

import { enableWebPush, keepOnlyPushWorker, keyBytes, PUSH_SW_SCOPE } from '@/lib/web-push'
import { wireWebPushSubscribe } from '@/wire/endpoints/web-push'

vi.mock('@/wire/endpoints/web-push', () => ({
  wireWebPushSubscribe: vi.fn(() => Promise.resolve()),
  wireWebPushUnsubscribe: vi.fn(() => Promise.resolve()),
}))

describe('web push setup', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    Reflect.deleteProperty(navigator, 'serviceWorker')
  })

  it('start-up removes every service worker but the push worker', async () => {
    const reg = (scope: string) => ({ scope, unregister: vi.fn(() => Promise.resolve(true)) })
    const old = reg('https://mail.test/')
    const push = reg(`https://mail.test${PUSH_SW_SCOPE}`)
    const sw = { getRegistrations: () => Promise.resolve([old, push]) }
    await keepOnlyPushWorker(sw as unknown as ServiceWorkerContainer)
    expect(old.unregister).toHaveBeenCalled()
    expect(push.unregister).not.toHaveBeenCalled()
  })

  it('decodes the VAPID key from base64url', () => {
    expect([...keyBytes('BP8A_w')]).toEqual([0x04, 0xff, 0x00, 0xff])
  })

  /**
   * The push worker's scope is /push-sw/, so it never controls a page, and
   * `navigator.serviceWorker.ready` (a worker for the page's own scope)
   * never settles. Waiting on it kept "Turn on in this browser" spinning
   * in every browser, and no browser ever subscribed.
   */
  it('subscribes once the push worker is active, without waiting for a worker to control the page', async () => {
    const worker = new EventTarget() as EventTarget & { state: string }
    worker.state = 'installing'
    const subscribe = vi.fn(() =>
      Promise.resolve({
        endpoint: 'https://fcm.googleapis.com/fcm/send/x',
        toJSON: () => ({ keys: { auth: 'a', p256dh: 'p' } }),
      })
    )
    const reg = { active: null, installing: worker, pushManager: { subscribe }, waiting: null }
    vi.stubGlobal('Notification', { requestPermission: () => Promise.resolve('granted') })
    Object.defineProperty(navigator, 'serviceWorker', {
      configurable: true,
      value: {
        ready: new Promise(() => undefined),
        register: () => {
          // the worker activates after registration resolves
          setTimeout(() => {
            worker.state = 'activated'
            worker.dispatchEvent(new Event('statechange'))
          }, 0)
          return Promise.resolve(reg)
        },
      },
    })

    await expect(enableWebPush('BP8A_w')).resolves.toBe('subscribed')
    expect(subscribe).toHaveBeenCalledTimes(1)
    expect(wireWebPushSubscribe).toHaveBeenCalledWith({
      endpoint: 'https://fcm.googleapis.com/fcm/send/x',
      keys: { auth: 'a', p256dh: 'p' },
      resume: true,
    })
  }, 2000)
})
