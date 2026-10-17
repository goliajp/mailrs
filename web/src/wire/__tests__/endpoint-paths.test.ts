import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * `wireFetch` puts `/api` in front of every path itself. A path that
 * starts with `/api/` goes to `/api/api/…`, which the server answered
 * with the web app's HTML and a 200: the DMARC page failed that way.
 */
const ENDPOINTS = join(import.meta.dirname, '..', 'endpoints')

describe('wire endpoint paths', () => {
  it('never carry the /api prefix wireFetch adds', () => {
    const offenders = readdirSync(ENDPOINTS)
      .filter((f) => f.endsWith('.ts'))
      .flatMap((f) =>
        readFileSync(join(ENDPOINTS, f), 'utf8')
          .split('\n')
          .map((line, i) => ({ at: `${f}:${i + 1}`, line }))
      )
      .filter(({ line }) => /path:\s*[`'"]\/api\//.test(line))
      .map(({ at }) => at)
    expect(offenders).toEqual([])
  })
})
