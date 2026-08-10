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

  it('sender list add', async () => {
    const { wireAddSender } = await import('../endpoints/settings')
    const body = await bodyOf(() => wireAddSender('whitelist', 'friend@example.com'))
    expect(body).toEqual(fixture('sender-list-add'))
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
        basicAuthPass: 'hunter2',
        basicAuthUser: 'team',
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

  /**
   * The reason the AI fixtures exist: this call sent `sender` and `subject`,
   * the handler reads `original_sender` and `original_subject`, and serde
   * dropped the two it did not recognise — leaving a required field missing
   * and every Suggest a 422.
   */
  it('ai reply suggest', async () => {
    const { wireReplySuggest } = await import('../endpoints/ai')
    const body = await bodyOf(() =>
      wireReplySuggest({
        original_body: 'Are you free on Thursday?',
        original_sender: 'nagata@nagatax.tokyo.jp',
        original_subject: 'Meeting',
        thread_context: 'From: nagata@nagatax.tokyo.jp\nearlier message',
      })
    )
    expect(body).toEqual(fixture('ai-reply-suggest'))
  })

  it('ai polish', async () => {
    const { wirePolishText } = await import('../endpoints/ai')
    const body = await bodyOf(() => wirePolishText('please make this better', 'professional'))
    expect(body).toEqual(fixture('ai-polish'))
  })

  it('ai generate subject', async () => {
    const { wireGenerateSubject } = await import('../endpoints/ai')
    const body = await bodyOf(() =>
      wireGenerateSubject({
        body: 'Confirming Thursday at 3pm.',
        context: 'To: nagata@nagatax.tokyo.jp',
      })
    )
    expect(body).toEqual(fixture('ai-generate-subject'))
  })

  /**
   * The autosave, every three seconds while composing. Its payload was
   * `Record<string, unknown>` until 2026-07-31, so a renamed field compiled
   * and serde dropped it on arrival.
   */
  it('draft save', async () => {
    const { wireSaveDraft } = await import('../endpoints/mail')
    const body = await bodyOf(() =>
      wireSaveDraft({
        bcc: '',
        body: 'Confirming Thursday at 3pm.',
        cc: 'someone@example.com',
        id: 42,
        reply_to_thread_id: 'a48529b44b1b190f@golia.jp',
        subject: 'Re: Meeting',
        to: 'nagata@nagatax.tokyo.jp',
      })
    )
    expect(body).toEqual(fixture('draft-save'))
  })

  /**
   * Alias creation, the admin write with the worst failure mode: every
   * non-account address on these domains resolves through an alias, so a
   * dropped field is mail that goes nowhere.
   *
   * Admin writes bypass the wire layer's typed functions — the pages call
   * `adminPost` with an inline object — so nothing checked any of the
   * thirteen of them until 2026-07-31. The body here is the one
   * `admin-aliases.tsx` builds.
   */
  it('alias create', async () => {
    const { adminPost } = await import('../endpoints/admin')
    const body = await bodyOf(() =>
      adminPost('/admin/aliases', {
        alias_type: 'forward',
        domain: 'golia.jp',
        source_address: 'devops@golia.jp',
        target_address: 'lihao@golia.jp',
      })
    )
    expect(body).toEqual(fixture('alias-create'))
  })

  /**
   * Account provisioning. `password` is hashed server-side, so a dropped
   * field stores an account with no usable credential rather than failing.
   */
  it('account create', async () => {
    const { adminPost } = await import('../endpoints/admin')
    const body = await bodyOf(() =>
      adminPost('/admin/accounts', {
        address: 'qa@golia.jp',
        display_name: 'QA',
        password: 'not-a-real-password',
      })
    )
    expect(body).toEqual(fixture('account-create'))
  })

  it('domain create', async () => {
    const { adminPost } = await import('../endpoints/admin')
    const body = await bodyOf(() => adminPost('/admin/domains', { name: 'golia.jp' }))
    expect(body).toEqual(fixture('domain-create'))
  })

  /**
   * A 405 in production until 2026-07-31 — the lane registered POST while
   * this page sends PUT — so the body had never reached the handler.
   */
  it('group permissions set', async () => {
    const { adminPut } = await import('../endpoints/admin')
    const body = await bodyOf(() =>
      adminPut('/admin/groups/1/permissions', {
        permissions: ['admin.accounts', 'admin.aliases'],
      })
    )
    expect(body).toEqual(fixture('group-permissions-set'))
  })

  it('login', async () => {
    const { wireLogin } = await import('../endpoints/auth')
    const body = await bodyOf(() => wireLogin('lihao@golia.jp', 'not-a-real-password'))
    expect(body).toEqual(fixture('login'))
  })

  it('change password', async () => {
    const { wireChangePassword } = await import('../endpoints/auth')
    const body = await bodyOf(() =>
      wireChangePassword('not-a-real-old-password', 'not-a-real-new-password')
    )
    expect(body).toEqual(fixture('change-password'))
  })

  it('reset password', async () => {
    const { wireResetPassword } = await import('../endpoints/auth')
    const body = await bodyOf(() =>
      wireResetPassword('0197f3c2-4a1b-7d31-9e55-2c8a1f0b6d44', 'not-a-real-password')
    )
    expect(body).toEqual(fixture('reset-password'))
  })

  /** One of the nine wrong on 2026-07-30: setting it on a new account threw. */
  it('recovery email set', async () => {
    const { wireSetRecoveryEmail } = await import('../endpoints/auth')
    const body = await bodyOf(() => wireSetRecoveryEmail('backup@example.com'))
    expect(body).toEqual(fixture('recovery-email-set'))
  })

  it('agent key create', async () => {
    const { wireCreateAgentKey } = await import('../endpoints/settings')
    const body = await bodyOf(() =>
      wireCreateAgentKey({ name: 'ci-bot', scopes: ['mail.read', 'mail.send'] })
    )
    expect(body).toEqual(fixture('agent-key-create'))
  })

  /**
   * The emoji is the key. One that arrives mangled is a reaction nobody can
   * remove, because removing it sends the same string back.
   */
  it('reaction toggle', async () => {
    const { wireToggleReaction } = await import('../endpoints/mail')
    const body = await bodyOf(() => wireToggleReaction('t1', 42, '\u{1F44D}'))
    expect(body).toEqual(fixture('reaction-toggle'))
  })

  it('account update', async () => {
    const { adminPut } = await import('../endpoints/admin')
    const address = encodeURIComponent('qa@golia.jp')
    const body = await bodyOf(() =>
      // Template form, so the coverage gate normalises this to the same
      // `/admin/accounts/{}` the page writes. A literal address would read
      // as a different route, and an interpolation containing a quote
      // breaks the scan that reads these paths.
      adminPut(`/admin/accounts/${address}`, { display_name: 'QA Team' })
    )
    expect(body).toEqual(fixture('account-update'))
  })

  it('group create', async () => {
    const { adminPost } = await import('../endpoints/admin')
    const body = await bodyOf(() =>
      adminPost('/admin/groups', {
        description: 'Full administrative access',
        name: 'admins',
      })
    )
    expect(body).toEqual(fixture('group-create'))
  })

  it('group members add', async () => {
    const { adminPost } = await import('../endpoints/admin')
    const body = await bodyOf(() =>
      adminPost('/admin/groups/1/members', { address: 'qa@golia.jp' })
    )
    expect(body).toEqual(fixture('group-members-add'))
  })

  it('email group members add', async () => {
    const { adminPost } = await import('../endpoints/admin')
    const body = await bodyOf(() =>
      adminPost('/admin/email-groups/1/members', { address: 'qa@golia.jp' })
    )
    expect(body).toEqual(fixture('email-group-members-add'))
  })

  /**
   * Enable and disable send the same one-field body to two handlers. Both
   * are asserted: two paths agreeing today is not one contract.
   */
  it('totp enable', async () => {
    const { wireTotpEnable } = await import('../endpoints/auth')
    const body = await bodyOf(() => wireTotpEnable('123456'))
    expect(body).toEqual(fixture('totp-code'))
  })

  it('totp disable', async () => {
    const { wireTotpDisable } = await import('../endpoints/auth')
    const body = await bodyOf(() => wireTotpDisable('123456'))
    expect(body).toEqual(fixture('totp-code'))
  })

  /**
   * A sieve script is line-oriented, so whitespace is part of it — a body
   * that arrives reflowed is a filter that no longer parses.
   */
  it('account sieve set', async () => {
    const { adminPost } = await import('../endpoints/admin')
    const address = encodeURIComponent('qa@golia.jp')
    const script =
      'require ["fileinto"];\nif header :contains "subject" "[devops]" {\n  fileinto "Notifications";\n}\n'
    const body = await bodyOf(() => adminPost(`/admin/accounts/${address}/sieve`, { script }))
    expect(body).toEqual(fixture('account-sieve-set'))
  })

  it('email group create', async () => {
    const { adminPost } = await import('../endpoints/admin')
    const body = await bodyOf(() =>
      adminPost('/admin/email-groups', {
        address: 'team@golia.jp',
        description: 'engineering',
        domain: 'golia.jp',
        name: 'Team',
      })
    )
    expect(body).toEqual(fixture('email-group-create'))
  })

  /** `note` is null rather than absent — the handler distinguishes them. */
  it('greylist local list add', async () => {
    const { adminPost } = await import('../endpoints/admin')
    const body = await bodyOf(() =>
      adminPost('/admin/greylist/local-lists', {
        kind: 'domain',
        list: 'blacklist',
        note: null,
        value: 'spam.example.com',
      })
    )
    expect(body).toEqual(fixture('greylist-local-add'))
  })

  /**
   * `{value}`, not a bare string. The handler read `body.as_str()` with the
   * whole document's JSON text as its fallback until 2026-07-31, so every
   * setting would have been stored as the literal `{"value":"..."}` — never
   * seen because the route took POST while this sends PUT and the request
   * was a 405.
   */
  it('system config set', async () => {
    const { adminPut } = await import('../endpoints/admin')
    const key = encodeURIComponent('smtp.banner')
    const body = await bodyOf(() => adminPut(`/admin/system-config/${key}`, { value: 'mailrs' }))
    expect(body).toEqual(fixture('system-config-set'))
  })

  it('totp setup', async () => {
    const { wireTotpSetup } = await import('../endpoints/auth')
    const body = await bodyOf(() => wireTotpSetup())
    expect(body).toEqual({})
  })

  it('identity unlink', async () => {
    const { wireUnlinkIdentity } = await import('../endpoints/auth')
    const body = await bodyOf(() => wireUnlinkIdentity('https://accounts.google.com', '1029384756'))
    expect(body).toEqual(fixture('identity-unlink'))
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
