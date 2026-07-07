import { afterEach, describe, expect, it, vi } from 'vitest'
import { z } from 'zod'

import { wireFetch } from '../client'
import { isWireError, WireErrorException } from '../errors'

vi.mock('@/store/auth', () => ({
  getToken: () => 'test-token',
}))

const SCHEMA = z.object({ ok: z.boolean() })

function respond(body: unknown, init?: ResponseInit): typeof fetch {
  return vi.fn(async () => new Response(JSON.stringify(body), init))
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('wireFetch', () => {
  it('parses a well-formed response through the schema', async () => {
    vi.stubGlobal('fetch', respond({ ok: true }))
    const out = await wireFetch(SCHEMA, { path: '/hi' })
    expect(out).toEqual({ ok: true })
  })

  it('sends Authorization header from the auth store', async () => {
    const mock = respond({ ok: true })
    vi.stubGlobal('fetch', mock)
    await wireFetch(SCHEMA, { path: '/hi' })
    // Cast so we can peek at the call args
    const call = vi.mocked(mock).mock.calls[0]
    const opts = call[1] as { headers: Record<string, string> }
    expect(opts.headers.Authorization).toBe('Bearer test-token')
  })

  it('throws a validation WireError on schema mismatch', async () => {
    vi.stubGlobal('fetch', respond({ oops: 1 }))
    await expect(wireFetch(SCHEMA, { path: '/hi' })).rejects.toSatisfy(
      (err: unknown) => isWireError(err) && err.detail.kind === 'validation'
    )
  })

  it('maps 401 to WireError.kind=auth', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('{}', { status: 401 }))
    )
    await expect(wireFetch(SCHEMA, { path: '/hi' })).rejects.toSatisfy(
      (err: unknown) => isWireError(err) && err.detail.kind === 'auth'
    )
  })

  it('maps 403 to WireError.kind=forbidden', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('{}', { status: 403 }))
    )
    await expect(wireFetch(SCHEMA, { path: '/hi' })).rejects.toSatisfy(
      (err: unknown) => isWireError(err) && err.detail.kind === 'forbidden'
    )
  })

  it('maps 404 to WireError.kind=not-found', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('{}', { status: 404 }))
    )
    await expect(wireFetch(SCHEMA, { path: '/hi' })).rejects.toSatisfy(
      (err: unknown) => isWireError(err) && err.detail.kind === 'not-found'
    )
  })

  it('lifts server body.error into WireError.message', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify({ error: 'bad input' }), { status: 422 }))
    )
    try {
      await wireFetch(SCHEMA, { path: '/hi' })
      expect.fail('should have thrown')
    } catch (err) {
      if (!isWireError(err)) throw err
      expect(err.detail.kind).toBe('server')
      if (err.detail.kind === 'server') {
        expect(err.detail.status).toBe(422)
        expect(err.detail.message).toBe('bad input')
      }
    }
  })

  it('resolves undefined for a 204 when allowEmpty is set', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(null, { status: 204 }))
    )
    const out = await wireFetch(SCHEMA.optional(), { allowEmpty: true, path: '/hi' })
    expect(out).toBeUndefined()
  })

  it('maps network failure to WireError.kind=network', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        throw new TypeError('network')
      })
    )
    await expect(wireFetch(SCHEMA, { path: '/hi' })).rejects.toSatisfy(
      (err: unknown) => isWireError(err) && err.detail.kind === 'network'
    )
  })

  it('maps AbortError to WireError.kind=aborted', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        throw new DOMException('aborted', 'AbortError')
      })
    )
    await expect(wireFetch(SCHEMA, { path: '/hi' })).rejects.toSatisfy(
      (err: unknown) => isWireError(err) && err.detail.kind === 'aborted'
    )
  })

  it('exposes WireErrorException with detail on thrown error', () => {
    const e = new WireErrorException({ kind: 'auth' })
    expect(e.detail.kind).toBe('auth')
  })
})
