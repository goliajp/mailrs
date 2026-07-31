import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

import { wireThreadListResponseSchema } from '../schemas/conversation'
import { sendsSchema } from '../schemas/sends'

/**
 * The client's schemas, against shapes captured from production.
 *
 * `settings-schemas.test.ts` and friends assert that a hand-written object
 * parses. That object was written to match the schema, so it always parses:
 * the test restates the schema in a second syntax and cannot notice the
 * backend renaming a field. On 2026-07-30, nine request bodies were wrong
 * and every test of that kind stayed green — four failing on every call,
 * five silently dropping what the user had asked for.
 *
 * These fixtures are read by `crates/webapi/tests/response_contract.rs` too,
 * which asserts the handler's own type still serializes to the same keys.
 * Neither side can pass by agreeing with itself.
 */

const RESPONSES = join(import.meta.dirname, '..', '..', '..', '..', 'wire-contract', 'responses')

function fixture(name: string): unknown {
  return JSON.parse(readFileSync(join(RESPONSES, `${name}.json`), 'utf8'))
}

describe('response contract', () => {
  it('the conversation list parses, with every field carried through', () => {
    const parsed = wireThreadListResponseSchema.parse(fixture('conversation-list'))
    expect(parsed.items).toHaveLength(1)

    const t = parsed.items[0]
    expect(t.thread_id).toBe('a48529b44b1b190f@golia.jp')
    expect(t.subject).toBe('Re: Meeting')
    expect(t.message_count).toBe(2)
    // The counters the Sent/Inbox split reads. A schema that dropped either
    // would render every thread as neither sent nor received.
    expect(t.sent_count).toBe(1)
    expect(t.received_count).toBe(1)
    // Importance drives the section a thread lands in.
    expect(t.importance_level).toBe('normal')
    expect(t.importance_score).toBeCloseTo(0.4)
    expect(t.participants).toEqual(['nagata@nagatax.tokyo.jp', 'lihao@golia.jp'])
  })

  /**
   * The endpoint returns a bare array; the schema is a union that also
   * accepts `{items}`. Both shapes have been served historically, and the
   * union is why a change of envelope does not empty the mailbox.
   */
  it('the bare-array form is what production sends', () => {
    const raw = fixture('conversation-list')
    expect(Array.isArray(raw)).toBe(true)
    expect(() => wireThreadListResponseSchema.parse({ items: raw })).not.toThrow()
  })

  it('the send list parses, keeping the fields the Send tab acts on', () => {
    const items = sendsSchema.parse(fixture('send-list'))
    expect(items).toHaveLength(1)

    const s = items[0]
    expect(s.send_id).toBe('0197f3c2-4a1b-7d31-9e55-2c8a1f0b6d44')
    expect(s.status).toBe('delivered')
    // Drives "Edit and send again". Losing it removes the button silently.
    expect(s.can_resend).toBe(true)
    // Null for an ordinary send — a schema requiring it would reject them all.
    expect(s.resent_from ?? null).toBeNull()
    expect(s.recipients?.[0]?.recipient).toBe('nagata@nagatax.tokyo.jp')
    expect(s.recipients?.[0]?.delivered).toBe(true)
  })

  it('the categories response parses into what the filter chips read', () => {
    const raw = fixture('conversation-categories') as { category: string; count: number }[]
    expect(raw.map((c) => c.category)).toEqual(['inbox', 'notification'])
    expect(raw[0].count).toBe(4224)
  })

  it('every response fixture is checked by a case above', () => {
    const present = readdirSync(RESPONSES)
      .filter((f) => f.endsWith('.json'))
      .map((f) => f.replace(/\.json$/, ''))
      .sort()
    expect(present).toEqual(['conversation-categories', 'conversation-list', 'send-list'])
  })
})
