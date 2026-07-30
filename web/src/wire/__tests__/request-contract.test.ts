import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

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

function fixture(name: string): unknown {
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
async function bodyOf(call: () => Promise<unknown>): Promise<unknown> {
  try {
    await call()
  } catch (e) {
    const kind = (e as { detail?: { kind?: string } })?.detail?.kind
    if (kind !== 'validation') throw e
  }
  expect(captured.body, 'the call never sent a request body').toBeDefined()
  return captured.body
}

describe('request bodies match the shared contract', () => {
  it('snooze', async () => {
    const { wireSnoozeConversation } = await import('../endpoints/mail')
    const body = await bodyOf(() => wireSnoozeConversation('t1', 1_785_542_400))
    expect(body).toEqual(fixture('snooze'))
    expect(captured.method).toBe('PUT')
  })

  it('feedback', async () => {
    const { wireRecordFeedback } = await import('../endpoints/mail')
    const body = await bodyOf(() => wireRecordFeedback('someone@example.com', 'block'))
    expect(body).toEqual(fixture('feedback'))
  })

  it('signature save', async () => {
    const { wireCreateSignature } = await import('../endpoints/settings')
    const body = await bodyOf(() =>
      wireCreateSignature({
        html_content: '<p>Regards</p>',
        name: 'default',
        text_content: 'Regards',
      })
    )
    // `id` is absent in the fixture and undefined here; JSON.stringify drops
    // it, so the captured body has no such key either.
    expect(body).toEqual(fixture('signature-save'))
  })

  it('key upload', async () => {
    const { wireUploadKey } = await import('../endpoints/settings')
    const body = await bodyOf(() =>
      wireUploadKey(
        'pgp',
        '-----BEGIN PGP PUBLIC KEY BLOCK-----\nabc\n-----END PGP PUBLIC KEY BLOCK-----'
      )
    )
    expect(body).toEqual(fixture('key-upload'))
  })

  it('webhook create', async () => {
    const { wireCreateWebhook } = await import('../endpoints/settings')
    const body = await bodyOf(() =>
      wireCreateWebhook({
        event_type: 'message.received',
        filter_sender: 'alerts@example.com',
        filter_thread_id: null,
        url: 'https://hooks.example.com/mailrs',
      })
    )
    expect(body).toEqual(fixture('webhook-create'))
  })

  it('calendar feed create', async () => {
    const { wireCreateCalendarFeed } = await import('../endpoints/settings')
    const body = await bodyOf(() =>
      wireCreateCalendarFeed({
        name: 'Team calendar',
        url: 'https://cal.example.com/team.ics',
      })
    )
    expect(body).toEqual(fixture('calendar-feed-create'))
  })

  it('batch mutation', async () => {
    const { wireBatchMutation } = await import('../endpoints/mutations')
    const body = await bodyOf(() => wireBatchMutation('archive', ['t1@golia.jp', 't2@golia.jp']))
    expect(body).toEqual(fixture('batch-mutation'))
  })

  it('forgot password', async () => {
    const { wireForgotPassword } = await import('../endpoints/auth')
    const body = await bodyOf(() => wireForgotPassword('lihao@golia.jp', 'backup@example.com'))
    expect(body).toEqual(fixture('forgot-password'))
  })
})

describe('send bodies match the shared contract', () => {
  /**
   * `sendMail` picks its transport by whether there are attachments; with
   * none it sends JSON, which is the path these fixtures describe. The
   * multipart path is a FormData body and is covered by the field-name
   * assertions in the Rust half plus `send-mail-schedule.test.ts`.
   */
  it('send with a scheduled time', async () => {
    const { sendMail } = await import('@/lib/send-mail')
    const body = await bodyOf(() =>
      sendMail({
        bcc: [],
        body: 'hello',
        cc: [],
        from: 'lihao@golia.jp',
        htmlBody: '<p>hello</p>',
        scheduledAt: 1_785_542_400,
        subject: 'subject',
        to: ['someone@example.com'],
        token: 'test-token',
      })
    )
    expect(body).toEqual(fixture('send'))
  })

  it('send as a redraft, keeping two of the carried attachments', async () => {
    const { sendMail } = await import('@/lib/send-mail')
    const body = await bodyOf(() =>
      sendMail({
        bcc: [],
        body: 'fixed',
        cc: [],
        from: 'lihao@golia.jp',
        htmlBody: '<p>fixed</p>',
        redraftKeep: [0, 2],
        redraftOf: 'abc123@golia.jp',
        subject: 'subject',
        to: ['someone@example.com'],
        token: 'test-token',
      })
    )
    expect(body).toEqual(fixture('send-redraft'))
  })
})
