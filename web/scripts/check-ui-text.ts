/**
 * Interface text written straight into a component, not looked up with
 * `t()`: JSX text and the `aria-label`, `placeholder`, `title` and `label`
 * attributes given as a literal. Counted per file against
 * `ui-text-baseline.txt`; a file over its count, or a new file with any,
 * fails. Moving a file to `t()` lowers its count: run with `--update` to
 * write the lower counts (an update never raises one).
 */

import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { join, relative } from 'node:path'

const ROOT = join(import.meta.dir, '..')
const SRC = join(ROOT, 'src')
const BASELINE = join(import.meta.dir, 'ui-text-baseline.txt')

// words that read the same in every language: product and protocol names
const KEEP = new Set([
  'IMAP',
  'SMTP',
  'POP3',
  'DAV',
  'CalDAV',
  'CardDAV',
  'MCP',
  'DKIM',
  'SPF',
  'DMARC',
  'MBOX',
  'mailrs',
])

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name)
    if (statSync(path).isDirectory()) return name === '__tests__' ? [] : files(path)
    return path.endsWith('.tsx') ? [path] : []
  })
}

function isText(s: string): boolean {
  const words = s.trim()
  if (!/[A-Za-z]{2,}/.test(words)) return false
  return !words.split(/\s+/).every((w) => KEEP.has(w))
}

export function count(source: string): number {
  let n = 0
  for (const m of source.matchAll(/>([^<>{}]+)</g)) if (isText(m[1])) n++
  for (const m of source.matchAll(/\b(?:aria-label|placeholder|title|label)="([^"]*)"/g)) {
    if (isText(m[1])) n++
  }
  return n
}

function read(): Map<string, number> {
  const out = new Map<string, number>()
  let text = ''
  try {
    text = readFileSync(BASELINE, 'utf8')
  } catch {
    return out
  }
  for (const line of text.split('\n')) {
    const [path, n] = line.trim().split(/\s+/)
    if (path && n) out.set(path, Number(n))
  }
  return out
}

const baseline = read()
const now = new Map<string, number>()
for (const f of files(SRC)) {
  const n = count(readFileSync(f, 'utf8'))
  if (n > 0) now.set(relative(ROOT, f), n)
}

function write() {
  const lines = [...now].sort(([a], [b]) => a.localeCompare(b)).map(([p, n]) => `${p} ${n}`)
  writeFileSync(BASELINE, lines.join('\n') + '\n')
  console.log(
    `baseline: ${now.size} files, ${[...now.values()].reduce((a, b) => a + b, 0)} strings`
  )
}

// the first baseline is whatever is there today
if (baseline.size === 0 && process.argv.includes('--update')) {
  write()
  process.exit(0)
}

const over = [...now].filter(([path, n]) => n > (baseline.get(path) ?? 0))
if (over.length > 0) {
  console.error('!! interface text written into components; look it up with t() instead:')
  for (const [path, n] of over)
    console.error(`   ${path}: ${n} (allowed ${baseline.get(path) ?? 0})`)
  process.exit(1)
}

const lower = [...baseline].filter(([path, n]) => (now.get(path) ?? 0) < n)
if (process.argv.includes('--update')) {
  write()
} else if (lower.length > 0) {
  console.error(
    '!! fewer written-in strings than the baseline says; run `bun scripts/check-ui-text.ts --update`:'
  )
  for (const [path, n] of lower) console.error(`   ${path}: ${now.get(path) ?? 0} (baseline ${n})`)
  process.exit(1)
}
