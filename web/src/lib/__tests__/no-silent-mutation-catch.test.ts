import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * A write may not fail silently.
 *
 * Both draft autosaves wrapped their save in an empty `catch` under the
 * comment "transient — the next tick retries". That reading is true of a
 * dropped connection and false of a 422, which fails identically on every
 * tick forever — and on production a renamed request field did exactly that
 * while the user went on typing into a box that was not being saved. The
 * failure is invisible from the outside: no console entry, no toast, no
 * changed pixel.
 *
 * An empty catch around a *read* is a different matter. React Query refetches
 * on the next mount, and `invalidateQueries().catch(() => {})` losing a
 * refresh costs a stale list until the next interaction. Those stay allowed;
 * this checks the ones that discard the user's data.
 */

const SRC = join(import.meta.dirname, '..', '..')

/** Names that write. A `catch` swallowing one of these loses user input. */
const MUTATION_CALLS = [
  'mutateAsync',
  'mutate(',
  'wireSaveDraft',
  'wireDeleteDraft',
  'wireSendMail',
  'wireMarkThreadRead',
  'postJson',
  'putJson',
  'patchJson',
  'deleteJson',
]

const EMPTY_CATCH = /catch\s*(\([A-Za-z_$][\w$]*\)\s*)?\{\s*(\/\/[^\n]*\n\s*)*\}/g

function sources(dir: string, out: string[] = []): string[] {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name)
    if (e.isDirectory()) {
      if (e.name !== '__tests__' && e.name !== 'node_modules') sources(p, out)
    } else if (/\.tsx?$/.test(e.name) && !/\.test\.tsx?$/.test(e.name)) {
      out.push(p)
    }
  }
  return out
}

describe('no silent mutation catch', () => {
  it('finds sources to scan', () => {
    expect(sources(SRC).length).toBeGreaterThan(50)
  })

  it('no empty catch encloses a write', () => {
    const offences: string[] = []

    for (const path of sources(SRC)) {
      const text = readFileSync(path, 'utf8')
      for (const m of text.matchAll(EMPTY_CATCH)) {
        // The try block this catch belongs to: everything from the previous
        // `try {` up to the catch. Approximated by the preceding 40 lines,
        // which covers every autosave-shaped block in this tree.
        const before = text.slice(0, m.index)
        const tryStart = before.lastIndexOf('try {')
        const block =
          tryStart === -1 ? before.split('\n').slice(-40).join('\n') : before.slice(tryStart)
        const call = MUTATION_CALLS.find((c) => block.includes(c))
        if (call === undefined) continue
        const line = before.split('\n').length
        offences.push(`${path.slice(SRC.length + 1)}:${line} swallows ${call}`)
      }
    }

    expect(
      offences,
      'an empty catch around a write discards the failure and the user is never told — ' +
        'route it through `useAutosaveStatus` (periodic saves) or surface a toast (one-shot actions)'
    ).toEqual([])
  })

  it('detects the pattern it is meant to catch', () => {
    // The shape as it was written, so a change to the regex that stops
    // matching it fails here rather than passing everywhere.
    const sample = `
      try {
        const res = await saveDraftMut.mutateAsync({ body })
        lastSavedRef.current = snapshot
      } catch {
        // transient — the next interval tick retries
      }`
    const hits = [...sample.matchAll(EMPTY_CATCH)]
    expect(hits).toHaveLength(1)
    expect(sample.slice(0, hits[0].index)).toContain('mutateAsync')
  })
})
