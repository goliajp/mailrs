import { describe, expect, it, vi } from 'vitest'

import { keepOnlyPushWorker, keyBytes, PUSH_SW_SCOPE } from '@/lib/web-push'

describe('web push setup', () => {
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
})
