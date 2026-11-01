import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { runInNewContext } from 'node:vm'
import { describe, expect, it, vi } from 'vitest'

/** Fire an event whose work goes through `waitUntil`, and wait for it. */
async function fire(fn: (event: unknown) => void, event: Record<string, unknown>) {
  let work: Promise<unknown> = Promise.resolve()
  fn({ ...event, waitUntil: (p: Promise<unknown>) => (work = p) })
  await work
}

/** Load public/push-sw.js against a stub worker scope; its listeners come back. */
function loadWorker(
  windows: { focus: () => Promise<void>; navigate: (u: string) => Promise<void>; url: string }[]
) {
  const listeners: Record<string, (event: unknown) => void> = {}
  const shown: { options: { body?: string; data: unknown; tag: string }; title: string }[] = []
  const opened: string[] = []
  const fetches: { body: string; url: string }[] = []
  const self = {
    clients: {
      matchAll: () => Promise.resolve(windows),
      openWindow: (url: string) => {
        opened.push(url)
        return Promise.resolve()
      },
    },
    location: { origin: 'https://mail.test' },
    navigator: { language: 'zh-CN' },
    registration: {
      showNotification: (title: string, options: never) => {
        shown.push({ options, title })
        return Promise.resolve()
      },
    },
    addEventListener: (type: string, fn: (event: unknown) => void) => {
      listeners[type] = fn
    },
    skipWaiting: () => undefined,
  }
  const fetch = (url: string, init: { body: string }) => {
    fetches.push({ body: init.body, url })
    return Promise.resolve(new Response(null, { status: 204 }))
  }
  const src = readFileSync(join(import.meta.dirname, '../../../public/push-sw.js'), 'utf8')
  runInNewContext(src, { fetch, self, URL })
  return { fetches, listeners, opened, shown }
}

describe('push service worker', () => {
  it('shows sender and subject, or only that mail came when content is hidden', async () => {
    const w = loadWorker([])
    const payload = (data: unknown) => ({ data: { json: () => data } })
    await fire(
      w.listeners.push,
      payload({ notification_id: 'n1', sender: 'Boss', subject: 'Raise', thread_id: 't1' })
    )
    await fire(w.listeners.push, payload({ hidden: true, notification_id: 'n2', thread_id: 't2' }))
    expect(w.shown[0].title).toBe('Boss')
    expect(w.shown[0].options.body).toBe('Raise')
    expect(w.shown[0].options.tag).toBe('t1')
    expect(w.shown[1].title).toBe('新邮件')
    expect(w.shown[1].options.body).toBeUndefined()
    expect(JSON.stringify(w.shown[1])).not.toContain('Boss')
  })

  it('a click counts once and opens the conversation, in an open window when there is one', async () => {
    const navigate = vi.fn(() => Promise.resolve())
    const focus = vi.fn(() => Promise.resolve())
    const w = loadWorker([{ focus, navigate, url: 'https://mail.test/settings' }])
    const close = vi.fn()
    await fire(w.listeners.notificationclick, {
      notification: { close, data: { notification_id: 'n1', thread_id: 'a/b' } },
    })
    expect(close).toHaveBeenCalled()
    expect(w.fetches).toEqual([{ body: '{"notification_id":"n1"}', url: '/api/push/clicked' }])
    expect(focus).toHaveBeenCalled()
    expect(navigate).toHaveBeenCalledWith('/mail?thread=a%2Fb&view=thread')

    const fresh = loadWorker([])
    await fire(fresh.listeners.notificationclick, {
      notification: { close, data: { notification_id: 'n2', thread_id: 't9' } },
    })
    expect(fresh.opened).toEqual(['/mail?thread=t9&view=thread'])
  })
})
