/**
 * Interface text written straight into a component, not looked up with
 * `t()`: JSX text, and the `aria-label`, `placeholder`, `title`, `label`,
 * `alt` and `helperText` attributes given as a string literal. Read from
 * the syntax tree, so comments, generics and comparisons never count.
 * Counted per file against `ui-text-baseline.txt`; a file over its count,
 * or a new file with any, fails. Moving a file to `t()` lowers its count:
 * run with `--update` to write the lower counts (an update never raises
 * one).
 */

import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { join, relative } from 'node:path'
import ts from 'typescript'

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
  // status-bar and monitor labels
  'PG',
  'Kevy',
  'TLS',
  'web',
  'api',
  'CIDR',
  'ESC',
  'Esc',
])

// example input shown as a placeholder: code and query syntax, not prose
const SAMPLES = new Set(['require "fileinto"; ...', 'from:news@shop.example has:attachment'])

// sample content for the component showcase, not interface text
const EXEMPT = new Set(['src/pages/playground.tsx'])

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
  // an example address reads the same in every language
  if (/^[^\s@]+@[^\s@]+$/.test(words)) return false
  // one technical token: a domain, URL, identifier or HTML entity
  if (/^\S+$/.test(words) && /[.:/_&;-]/.test(words)) return false
  if (SAMPLES.has(words)) return false
  return !words.split(/\s+/).every((w) => KEEP.has(w))
}

const ATTRS = new Set(['alt', 'aria-label', 'helperText', 'label', 'placeholder', 'title'])

/** Each piece of written-in text in `source`, with its line. */
export function found(source: string): { line: number; text: string }[] {
  const file = ts.createSourceFile('x.tsx', source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
  const out: { line: number; text: string }[] = []
  const add = (node: ts.Node, text: string) =>
    out.push({ line: file.getLineAndCharacterOfPosition(node.getStart(file)).line + 1, text })
  const visit = (node: ts.Node) => {
    if (ts.isJsxText(node) && isText(node.text)) add(node, node.text.trim().replace(/\s+/g, ' '))
    if (
      ts.isJsxAttribute(node) &&
      ATTRS.has(node.name.getText(file)) &&
      node.initializer &&
      ts.isStringLiteral(node.initializer) &&
      isText(node.initializer.text)
    ) {
      add(node, `${node.name.getText(file)}="${node.initializer.text}"`)
    }
    ts.forEachChild(node, visit)
  }
  visit(file)
  return out
}

export function count(source: string): number {
  return found(source).length
}

// `--list <file>...` prints what would be counted in those files
const listAt = process.argv.indexOf('--list')
if (listAt >= 0) {
  for (const path of process.argv.slice(listAt + 1)) {
    console.log(`== ${path}`)
    for (const { line, text } of found(readFileSync(path, 'utf8')))
      console.log(`  ${line}: ${text}`)
  }
  process.exit(0)
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
  if (EXEMPT.has(relative(ROOT, f))) continue
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
