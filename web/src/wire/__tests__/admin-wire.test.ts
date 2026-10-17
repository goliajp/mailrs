import { afterEach, describe, expect, it, vi } from 'vitest'

import { adminPost, adminPut, adminStringListGet } from '../endpoints/admin'
import { adminStringListSchema } from '../schemas/admin'

function answer(status: number, body?: unknown) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () =>
      Promise.resolve(
        new Response(body === undefined ? null : JSON.stringify(body), {
          headers: { 'content-type': 'application/json' },
          status,
        })
      )
    )
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('admin writes', () => {
  // most admin writes answer 204; this used to throw after the write had landed
  it('accept an empty 204', async () => {
    answer(204)
    await expect(adminPost('/admin/domains', { name: 'x.test' })).resolves.toBeUndefined()
    answer(204)
    await expect(adminPut('/admin/system-config/k', { value: 'v' })).resolves.toBeUndefined()
  })

  it('still pass a body through when there is one', async () => {
    answer(200, { id: 3, secret: 's' })
    await expect(adminPost('/admin/apps', {})).resolves.toEqual({ id: 3, secret: 's' })
  })
})

describe('admin string lists', () => {
  it('read every wrapping the backend uses', () => {
    expect(adminStringListSchema.parse(['a'])).toEqual(['a'])
    expect(adminStringListSchema.parse({ items: ['a', 'b'] })).toEqual(['a', 'b'])
    expect(adminStringListSchema.parse({ permissions: ['admin.domains'] })).toEqual([
      'admin.domains',
    ])
    expect(adminStringListSchema.parse({ members: [] })).toEqual([])
    expect(adminStringListSchema.safeParse({ items: [1] }).success).toBe(false)
  })

  it('come back as a plain array', async () => {
    answer(200, { groups: ['ops'] })
    await expect(adminStringListGet('/admin/accounts/a@b.test/groups')).resolves.toEqual(['ops'])
  })
})
