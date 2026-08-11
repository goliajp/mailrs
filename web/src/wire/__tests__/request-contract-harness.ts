import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { afterEach, beforeEach, expect, vi } from 'vitest'

/**
 * Every fixture in `wire-contract/requests/` must be what the real endpoint
 * function produces.
 *
 * The other half is `crates/webapi/tests/request_contract.rs`, which
 * deserializes the same files into the structs the handlers read. One file
 * is the contract and each side is checked against *it* — change one side
 * without the other and one of the two goes red.
 *
 * Why: an audit on 2026-07-30 found nine of thirty-five request bodies
 * wrong. Tests existed and did not help. `api.test.ts` asserted the snooze
 * body was `{until: <ISO>}` and passed every run while every snooze in
 * production answered 422, because it pinned what this side had decided to
 * send. Checking one side against itself proves nothing about the other.
 *
 * The endpoint functions are called for real; only the transport is stubbed.
 * Rebuilding the body by hand here would reintroduce exactly that problem.
 */

export function fixture(name: string): unknown {
  const path = join(__dirname, '../../../../wire-contract/requests', `${name}.json`)
  return JSON.parse(readFileSync(path, 'utf8'))
}

let captured: { body?: unknown; method?: string; path?: string } = {}

beforeEach(() => {
  captured = {}
  vi.mocked(globalThis.fetch)
  vi.stubGlobal(
    'fetch',
    vi.fn((input: string, init?: RequestInit) => {
      captured = {
        body: init?.body ? JSON.parse(init.body as string) : undefined,
        method: init?.method,
        path: input,
      }
      return Promise.resolve(
        new Response(JSON.stringify({ items: [], reactions: [], success: true }), {
          headers: { 'content-type': 'application/json' },
          status: 200,
        })
      )
    })
  )
})

afterEach(() => {
  vi.unstubAllGlobals()
})

vi.mock('@/store/auth', () => ({
  getToken: () => 'test-token',
}))

/**
 * Run a call and return the body it put on the wire.
 *
 * Response-schema failures are tolerated: the stub cannot satisfy ten
 * different response schemas at once, and this file is about the request.
 * A *missing* body is not tolerated — a call that never reached fetch would
 * otherwise pass silently, which is the failure mode this whole file exists
 * to prevent.
 */
export async function bodyOf(call: () => Promise<unknown>): Promise<unknown> {
  try {
    await call()
  } catch (e) {
    const kind = (e as { detail?: { kind?: string } })?.detail?.kind
    if (kind !== 'validation') throw e
  }
  expect(captured.body, 'the call never sent a request body').toBeDefined()
  return captured.body
}

/** The captured request, for the two suites that assert on it. */
export function capturedRequest(): { body?: unknown; method?: string; path?: string } {
  return captured
}
