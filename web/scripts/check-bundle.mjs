#!/usr/bin/env node
// Bundle-size budget gate.
//
// Reads dist/assets/*.js sizes (post-build), enforces:
//
//   1. Entry chunk (the one referenced by index.html) ≤ ENTRY_GZIP_BUDGET
//      bytes gzipped. This is what hits every cold load.
//   2. Total JS payload ≤ TOTAL_GZIP_BUDGET bytes gzipped. This is the
//      worst-case "user opens everything" cost.
//
// If either is exceeded the script exits non-zero so release.sh aborts
// before pushing the bloat. Adjust budgets after deliberate features —
// don't sneak past them.
//
// Tweakable via env: ENTRY_GZIP_BUDGET / TOTAL_GZIP_BUDGET (bytes).

import { gzipSync } from 'node:zlib'
import { readdirSync, readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const ENTRY_GZIP_BUDGET = Number(process.env.ENTRY_GZIP_BUDGET ?? 200 * 1024) // 200 KB
const TOTAL_GZIP_BUDGET = Number(process.env.TOTAL_GZIP_BUDGET ?? 2 * 1024 * 1024) // 2 MB

const root = resolve(import.meta.dirname, '..')
const assetsDir = resolve(root, 'dist/assets')
const indexHtml = readFileSync(resolve(root, 'dist/index.html'), 'utf-8')

// the entry chunk is whichever <script type=module src="..."> is in index.html
const entryMatch = indexHtml.match(/<script[^>]+type="module"[^>]+src="([^"]+)"/)
if (!entryMatch) {
  console.error('check-bundle: could not find entry script in dist/index.html')
  process.exit(2)
}
const entryFile = entryMatch[1].split('/').pop()

let entryGzip = 0
let totalGzip = 0
let entryRaw = 0
let totalRaw = 0
const rows = []

for (const f of readdirSync(assetsDir)) {
  if (!f.endsWith('.js')) continue
  const raw = readFileSync(resolve(assetsDir, f))
  const gz = gzipSync(raw).byteLength
  totalRaw += raw.byteLength
  totalGzip += gz
  if (f === entryFile) {
    entryRaw = raw.byteLength
    entryGzip = gz
  }
  rows.push({ file: f, gz, raw: raw.byteLength })
}

rows.sort((a, b) => b.gz - a.gz)
const fmt = (n) => `${(n / 1024).toFixed(1).padStart(7)} KB`

console.log('')
console.log('  bundle  (gzip)    (raw)    file')
console.log('  ──────  ───────  ───────  ────────────────────────────────────')
for (const r of rows.slice(0, 12)) {
  const marker = r.file === entryFile ? 'ENTRY ' : '      '
  console.log(`  ${marker}  ${fmt(r.gz)}  ${fmt(r.raw)}  ${r.file}`)
}
if (rows.length > 12) console.log(`  ...   (${rows.length - 12} more chunks)`)
console.log('')

const ok = (label, actual, budget) =>
  console.log(
    `  ${actual <= budget ? '✓' : '✗'} ${label}: ${fmt(actual)} / budget ${fmt(budget)}`
  )

ok(`entry (${entryFile})`, entryGzip, ENTRY_GZIP_BUDGET)
ok('total payload         ', totalGzip, TOTAL_GZIP_BUDGET)
console.log('')

if (entryGzip > ENTRY_GZIP_BUDGET) {
  console.error(
    `\n  entry chunk over budget by ${fmt(entryGzip - ENTRY_GZIP_BUDGET)}. ` +
      `move a heavy dep into a lazy boundary, or bump ENTRY_GZIP_BUDGET (and document why).\n`
  )
  process.exit(1)
}
if (totalGzip > TOTAL_GZIP_BUDGET) {
  console.error(
    `\n  total payload over budget by ${fmt(totalGzip - TOTAL_GZIP_BUDGET)}. ` +
      `review what was just added, or bump TOTAL_GZIP_BUDGET (and document why).\n`
  )
  process.exit(1)
}

void entryRaw
void totalRaw
