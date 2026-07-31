import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * Every wire call that sends a body is either checked against the shared
 * contract or listed as not yet checked, with a reason.
 *
 * A fixture directory answers "are these twelve bodies right?" and says
 * nothing about the twelve nobody wrote a fixture for — while looking, from
 * the outside, like request coverage. That is the shape of the 2026-07-30
 * audit: nine of thirty-five request bodies were wrong, four failing every
 * call and five silently dropping what the user had asked for, and the tests
 * that existed passed throughout because each checked the frontend against
 * itself.
 *
 * This does not check that a body is correct — `request-contract.test.ts`
 * and `crates/webapi/tests/request_contract.rs` do that, against one shared
 * file. This checks that the set of bodies being checked is the set of
 * bodies being sent.
 */

const ENDPOINTS = join(import.meta.dirname, '..', 'endpoints')
const FIXTURES = join(import.meta.dirname, '..', '..', '..', '..', 'wire-contract', 'requests')

/**
 * Functions that send a body and have no fixture yet, with why.
 *
 * A name here is a debt, not an exemption — the reason has to say what makes
 * it hard or why it does not apply, so the list can be worked down rather
 * than grown. Anything not listed and not covered fails the test below.
 */
const UNCOVERED: Record<string, string> = {
  wireChangePassword: 'sends a live credential; needs a fixture with a fake one',
  wireCreateAgentKey: 'admin surface, not yet enumerated',
  wireLogin: 'sends a live credential; needs a fixture with a fake one',
  wireLogout: 'sends an empty object; nothing to get wrong',
  wireMarkAllRead: 'sends an empty object; nothing to get wrong',
  wireResetPassword: 'sends a live credential; needs a fixture with a fake one',
  wireSendMailJson: 'covered by send.json; the multipart path is not',
  wireSetRecoveryEmail: 'admin surface, not yet enumerated',
  wireToggleReaction: 'admin surface, not yet enumerated',
  wireTotpDisable: 'TOTP flow, not yet enumerated',
  wireTotpEnable: 'TOTP flow, not yet enumerated',
  wireTotpSetup: 'TOTP flow, not yet enumerated',
}

/** Wire functions whose implementation contains a `body:`. */
function bodySenders(): string[] {
  const out: string[] = []
  for (const file of readdirSync(ENDPOINTS)) {
    if (!file.endsWith('.ts')) continue
    const src = readFileSync(join(ENDPOINTS, file), 'utf8')
    const decl = /export (?:const|async function|function) (wire[A-Za-z0-9_]+)/g
    for (const m of src.matchAll(decl)) {
      const next = src.indexOf('\nexport ', m.index + m[0].length)
      const body = src.slice(m.index, next === -1 ? src.length : next)
      if (/\bbody:/.test(body)) out.push(m[1])
    }
  }
  return out.sort()
}

/** Wire function names named by a case in the contract test. */
function covered(): Set<string> {
  const src = readFileSync(join(import.meta.dirname, 'request-contract.test.ts'), 'utf8')
  const names = new Set<string>()
  for (const m of src.matchAll(/const \{ (wire[A-Za-z0-9_]+) \} = await import/g)) {
    names.add(m[1])
  }
  return names
}

describe('request coverage', () => {
  it('finds the endpoints and the fixtures', () => {
    expect(bodySenders().length).toBeGreaterThan(15)
    expect(readdirSync(FIXTURES).filter((f) => f.endsWith('.json')).length).toBeGreaterThan(10)
  })

  it('every body-sending call is checked or listed with a reason', () => {
    const checked = covered()
    const unlisted = bodySenders().filter((fn) => !checked.has(fn) && UNCOVERED[fn] === undefined)

    expect(
      unlisted,
      'these send a request body that nothing checks against the backend struct — ' +
        'add a fixture to wire-contract/requests/ plus a case in ' +
        'request-contract.test.ts and crates/webapi/tests/request_contract.rs, ' +
        'or add the name to UNCOVERED with a reason'
    ).toEqual([])
  })

  it('nothing is listed as uncovered while actually being covered', () => {
    const checked = covered()
    const stale = Object.keys(UNCOVERED).filter((fn) => checked.has(fn))
    expect(stale, 'these are in UNCOVERED but a contract test does check them').toEqual([])
  })

  it('nothing is listed as uncovered that no longer sends a body', () => {
    const senders = new Set(bodySenders())
    const gone = Object.keys(UNCOVERED).filter((fn) => !senders.has(fn))
    expect(gone, 'these are in UNCOVERED but no longer send a body — remove them').toEqual([])
  })

  it('every reason says something', () => {
    for (const [fn, why] of Object.entries(UNCOVERED)) {
      expect(why.trim().length, `${fn} has no reason`).toBeGreaterThan(10)
    }
  })
})
