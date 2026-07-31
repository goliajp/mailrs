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
const SRC = join(import.meta.dirname, '..', '..')
const FIXTURES = join(import.meta.dirname, '..', '..', '..', '..', 'wire-contract', 'requests')

/**
 * Functions that send a body and have no fixture yet, with why.
 *
 * A name here is a debt, not an exemption — the reason has to say what makes
 * it hard or why it does not apply, so the list can be worked down rather
 * than grown. Anything not listed and not covered fails the test below.
 *
 * It has been worked down. What is left is not "not yet": two send an empty
 * object and one is the multipart send path, which needs a different fixture
 * shape than a JSON body. A fixture for a body with no fields would pin
 * nothing.
 */
const UNCOVERED: Record<string, string> = {
  wireLogout: 'sends an empty object; nothing to get wrong',
  wireMarkAllRead: 'sends an empty object; nothing to get wrong',
  wireSendMailJson: 'covered by send.json; the multipart path is not',
}

/**
 * Admin writes do not go through the wire layer at all — the admin pages
 * call `adminPost` / `adminPut` from `lib/api.ts`, whose body parameter is
 * untyped. Until this scanned for them the coverage gate reported every
 * body covered while thirteen went unchecked, which is the same
 * "looks like coverage" problem one level up.
 */
function adminWrites(): string[] {
  const out = new Set<string>()
  const walk = (dir: string) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, e.name)
      if (e.isDirectory()) {
        if (e.name !== '__tests__') walk(p)
      } else if (/\.tsx?$/.test(e.name) && !/\.test\.tsx?$/.test(e.name)) {
        const src = readFileSync(p, 'utf8')
        for (const m of src.matchAll(/admin(?:Post|Put|Patch)\(\s*[`']([^`']+)[`']/g)) {
          // Strip template holes so `/admin/accounts/${x}/sieve` and a
          // literal path collapse to one entry.
          out.add(normalisePath(m[1]))
        }
      }
    }
  }
  walk(SRC)
  return [...out].sort()
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

/** Admin paths named by a case in the contract test. */
function coveredAdminPaths(): Set<string> {
  const src = readFileSync(join(import.meta.dirname, 'request-contract.test.ts'), 'utf8')
  const out = new Set<string>()
  for (const m of src.matchAll(/admin(?:Post|Put|Patch)\(\s*[`']([^`']+)[`']/g)) {
    out.add(normalisePath(m[1]))
  }
  return out
}

/**
 * One spelling for a path with an id in it.
 *
 * A page writes `/admin/groups/${group.id}/permissions` and a test writes
 * `/admin/groups/1/permissions`; both name the same route, and comparing
 * them literally made a covered path read as uncovered.
 */
function normalisePath(p: string): string {
  return p.replace(/\$\{[^}]*\}/g, '{}').replace(/\/\d+(?=\/|$)/g, '/{}')
}

/**
 * Admin paths whose body nothing checks, with why.
 *
 * Same contract as UNCOVERED: a name here is debt, and the reason has to
 * say what it is waiting on. All four remaining send no body at all — the
 * state is in the path — so there is no shape to pin.
 */
const ADMIN_UNCOVERED: Record<string, string> = {
  '/conversations/{}/read{}': 'mark-read; sends no body, the state is the path',
  '/conversations/{}/star': 'star toggle; sends no body, the state is the path',
  '/conversations/{}/unread{}': 'mark-unread; sends no body, the state is the path',
  '/queue/{}/retry': 'retry a queued send; sends no body, the id is the path',
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

  it('every admin write is listed with a reason', () => {
    const checked = coveredAdminPaths()
    const unlisted = adminWrites().filter(
      (p) => !checked.has(p) && ADMIN_UNCOVERED[p] === undefined
    )
    expect(
      unlisted,
      'these admin pages send a body that nothing checks against the ' +
        'handler struct — add a fixture and a case on both sides, or list ' +
        'the path in ADMIN_UNCOVERED with a reason'
    ).toEqual([])
  })

  it('nothing is listed as an admin write that no longer exists', () => {
    const present = new Set(adminWrites())
    const gone = Object.keys(ADMIN_UNCOVERED).filter((p) => !present.has(p))
    expect(gone, 'these are in ADMIN_UNCOVERED but no page sends them').toEqual([])
  })

  it('nothing is listed as an admin write that a contract case does check', () => {
    const checked = coveredAdminPaths()
    const stale = Object.keys(ADMIN_UNCOVERED).filter((p) => checked.has(p))
    expect(stale, 'these are in ADMIN_UNCOVERED but are pinned by a fixture').toEqual([])
  })

  it('every reason says something', () => {
    for (const [fn, why] of Object.entries({ ...ADMIN_UNCOVERED, ...UNCOVERED })) {
      expect(why.trim().length, `${fn} has no reason`).toBeGreaterThan(10)
    }
  })
})
