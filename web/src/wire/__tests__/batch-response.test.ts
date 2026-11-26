import { afterEach, describe, expect, it, vi } from 'vitest'

import { wireBatchMutation } from '../endpoints/mutations'

afterEach(() => {
  vi.unstubAllGlobals()
})

function answer(body: unknown) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => new Response(JSON.stringify(body), { status: 200 }))
  )
}

// Backend: crates/webapi/src/handlers/conversation_verbs.rs — fn batch_mutation
describe('POST /conversations/batch', () => {
  it('reads the answer of a batch that went through, with or without a null message', async () => {
    // as the server sent it while `message` was serialized when empty
    answer({ failed: 0, message: null, processed: 41, success: true })
    const withNull = await wireBatchMutation('read', ['t1'])
    expect(withNull.success).toBe(true)
    expect(withNull.failed_thread_ids).toEqual([])

    answer({ failed: 0, processed: 41, success: true })
    expect((await wireBatchMutation('read', ['t1'])).processed).toBe(41)
  })

  it('names the conversations a batch could not do', async () => {
    answer({ failed: 1, failed_thread_ids: ['t2'], processed: 1, success: false })
    const got = await wireBatchMutation('archive', ['t1', 't2'])
    expect(got.success).toBe(false)
    expect(got.failed_thread_ids).toEqual(['t2'])
  })
})
