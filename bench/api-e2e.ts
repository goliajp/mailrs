#!/usr/bin/env bun
// bench/api-e2e.ts — hit every mailrs REST API listed in web/public/openapi.json,
// measure latency, capture status codes, surface the slowest endpoints.
//
// Usage:
//   MAILRS_BENCH_TOKEN=<bearer> bun run bench/api-e2e.ts [base-url]
//   MAILRS_BENCH_TOKEN=<bearer> MAILRS_BENCH_SAMPLES=10 bun run bench/api-e2e.ts
//
// Without MAILRS_BENCH_TOKEN: only public endpoints (health, well-known, etc.)
// are hit; authed endpoints get marked skip:no-token.
//
// Special: the "open a mail" critical-path probe runs at the end with extra
// timing detail per stage (list → thread → body), since that's the user-
// reported slow path.

import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

type Op = {
  method: string
  path: string
  summary: string
  authed: boolean
}

type Sample = { status: number; ms: number; bytes: number; err?: string }

type Result = { op: Op; samples: Sample[]; skipped?: string }

const BASE = process.argv[2] ?? 'https://mail.golia.ai'
const TOKEN = process.env.MAILRS_BENCH_TOKEN
const SAMPLES = Number(process.env.MAILRS_BENCH_SAMPLES ?? '5')
const TIMEOUT_MS = Number(process.env.MAILRS_BENCH_TIMEOUT_MS ?? '15000')

const OPENAPI_PATH = resolve(import.meta.dirname, '..', 'web', 'public', 'openapi.json')
const openapi = JSON.parse(readFileSync(OPENAPI_PATH, 'utf-8')) as {
  paths: Record<string, Record<string, { summary?: string; security?: unknown[] }>>
}

// ----- enumerate operations from openapi --------------------------------

function enumerateOps(): Op[] {
  const out: Op[] = []
  for (const [path, methods] of Object.entries(openapi.paths)) {
    for (const [method, op] of Object.entries(methods)) {
      const m = method.toLowerCase()
      if (!['get', 'post', 'put', 'delete', 'patch'].includes(m)) continue
      const authed = op.security === undefined || (Array.isArray(op.security) && op.security.length > 0)
      out.push({
        method: m.toUpperCase(),
        path,
        summary: op.summary ?? '',
        authed,
      })
    }
  }
  return out
}

// ----- path-param expansion -----------------------------------------------

function expandPath(p: string): string {
  return p
    .replaceAll(/\{[^}]*id[^}]*\}/gi, '1')
    .replaceAll(/\{[^}]*name[^}]*\}/gi, 'sample')
    .replaceAll(/\{[^}]+\}/g, 'sample')
}

// ----- skip rules — never hit anything that would mutate prod -------------

function skipReason(op: Op): string | null {
  if (op.method === 'DELETE') return 'mutation'
  if (op.method !== 'GET') {
    // allow specific read-shaped POST endpoints (search etc.); deny rest
    const allowPostShape = /\/(search|preview|render|test|validate|recipients)\b/
    if (!allowPostShape.test(op.path)) return 'mutation'
  }
  if (/\/auth\/(logout|change-password|reset-password|forgot-password|totp\/(setup|enable|disable))/.test(op.path)) {
    return 'auth-sensitive'
  }
  if (op.path === '/api/auth/login') return 'requires-body'
  if (/\/messages\/.*\/(move|flag|reply|forward|delete|move-trash|archive|unsnooze|snooze|pin|unpin|read|unread)/.test(op.path)) {
    return 'mutation'
  }
  if (/\/admin\/(export|import|reset|rebuild|reindex|purge|backfill|reload)/.test(op.path)) return 'mutation'
  if (op.authed && !TOKEN) return 'no-token'
  return null
}

// ----- fetch one endpoint N times -----------------------------------------

async function hit(op: Op): Promise<Sample[]> {
  const out: Sample[] = []
  const url = BASE + expandPath(op.path)
  for (let i = 0; i < SAMPLES; i++) {
    const headers: Record<string, string> = TOKEN ? { Authorization: `Bearer ${TOKEN}` } : {}
    const t0 = performance.now()
    let s: Sample = { status: 0, ms: 0, bytes: 0 }
    try {
      const res = await fetch(url, {
        method: op.method,
        headers,
        signal: AbortSignal.timeout(TIMEOUT_MS),
      })
      const buf = await res.arrayBuffer()
      s = { status: res.status, ms: performance.now() - t0, bytes: buf.byteLength }
    } catch (e) {
      s = { status: 0, ms: performance.now() - t0, bytes: 0, err: String(e) }
    }
    out.push(s)
  }
  return out
}

// ----- percentile + summary -----------------------------------------------

function pct(arr: number[], p: number): number {
  if (arr.length === 0) return 0
  const sorted = [...arr].sort((a, b) => a - b)
  const idx = Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1)
  return sorted[Math.max(0, idx)]
}

function fmt(n: number): string {
  return n.toFixed(0)
}

// ----- mail-open critical-path probe --------------------------------------
//
// Mirrors the frontend flow when a user clicks a conversation in the inbox:
//   1. list                — GET /api/conversations?folder=inbox&limit=50
//   2. thread              — GET /api/conversations/{thread_id}
//   3. body (largest msg)  — GET /api/mail/{message_id}/body
//
// Each stage timed independently so we can attribute the perceived lag.

async function probeMailOpen(): Promise<void> {
  if (!TOKEN) {
    console.log('\n## mail-open critical path\n')
    console.log('_skip (MAILRS_BENCH_TOKEN not set)_\n')
    return
  }
  console.log('\n## mail-open critical path\n')

  // stage 1
  const url1 = `${BASE}/api/conversations?folder=inbox&limit=50`
  const t1 = performance.now()
  const r1 = await fetch(url1, { headers: { Authorization: `Bearer ${TOKEN}` } })
  const list = (await r1.json()) as Array<{ thread_id?: string; id?: string; latest_message_id?: string }>
  const ms1 = performance.now() - t1
  console.log(`1. list inbox (limit=50)        ${fmt(ms1)} ms   status=${r1.status}   items=${Array.isArray(list) ? list.length : 'N/A'}`)
  if (!Array.isArray(list) || list.length === 0) {
    console.log('   _no conversations; cannot continue probe_\n')
    return
  }

  // pick a real thread id
  const threadId = list[0].thread_id ?? list[0].id
  if (!threadId) {
    console.log('   _first item missing thread_id/id; cannot continue probe_\n')
    return
  }

  // stage 2
  const url2 = `${BASE}/api/conversations/${encodeURIComponent(threadId)}`
  const t2 = performance.now()
  const r2 = await fetch(url2, { headers: { Authorization: `Bearer ${TOKEN}` } })
  const thread = (await r2.json()) as Array<{ id?: string | number; size?: number }>
  const ms2 = performance.now() - t2
  console.log(`2. thread ${threadId}                ${fmt(ms2)} ms   status=${r2.status}   messages=${Array.isArray(thread) ? thread.length : 'N/A'}`)
  if (!Array.isArray(thread) || thread.length === 0) {
    console.log('   _no messages in thread; cannot continue probe_\n')
    return
  }

  // stage 3 — pick the largest message (most realistic worst case)
  const largest = thread.reduce((a, b) => ((b.size ?? 0) > (a.size ?? 0) ? b : a))
  const msgId = largest.id
  if (msgId === undefined) {
    console.log('   _largest message missing id; cannot continue probe_\n')
    return
  }
  const url3 = `${BASE}/api/mail/${msgId}/body`
  const t3 = performance.now()
  const r3 = await fetch(url3, { headers: { Authorization: `Bearer ${TOKEN}` } })
  const bodyBuf = await r3.arrayBuffer()
  const ms3 = performance.now() - t3
  console.log(`3. message ${msgId} body              ${fmt(ms3)} ms   status=${r3.status}   bytes=${bodyBuf.byteLength}`)

  console.log(`\n   total ${fmt(ms1 + ms2 + ms3)} ms — list ${Math.round((ms1 / (ms1 + ms2 + ms3)) * 100)}% | thread ${Math.round((ms2 / (ms1 + ms2 + ms3)) * 100)}% | body ${Math.round((ms3 / (ms1 + ms2 + ms3)) * 100)}%\n`)
}

// ----- driver -------------------------------------------------------------

async function main(): Promise<void> {
  const ops = enumerateOps()
  console.error(`# discovered ${ops.length} operations across ${Object.keys(openapi.paths).length} paths`)
  console.error(`# base = ${BASE}, samples = ${SAMPLES}, token = ${TOKEN ? 'yes' : 'no'}\n`)

  const results: Result[] = []
  let i = 0
  for (const op of ops) {
    i++
    const reason = skipReason(op)
    if (reason) {
      results.push({ op, samples: [], skipped: reason })
      continue
    }
    process.stderr.write(`[${i}/${ops.length}] ${op.method} ${op.path}\r`)
    const samples = await hit(op)
    results.push({ op, samples })
  }
  process.stderr.write('\n')

  // ----- report header -----
  console.log(`# mailrs API e2e bench — ${BASE}\n`)
  console.log(`samples per op: ${SAMPLES}; token: ${TOKEN ? 'yes' : 'no'}\n`)

  // ----- ran ops, sorted by P95 desc -----
  const ran = results.filter((r) => r.samples.length > 0)
  ran.sort((a, b) => pct(b.samples.map((s) => s.ms), 95) - pct(a.samples.map((s) => s.ms), 95))

  console.log(`\n## endpoints ran (${ran.length}), sorted by P95 desc\n`)
  console.log('| Method | Path | Status | P50 (ms) | P95 (ms) | Max (ms) | Bytes (P50) |')
  console.log('|---|---|---|---:|---:|---:|---:|')
  for (const r of ran) {
    const ms = r.samples.map((s) => s.ms)
    const by = r.samples.map((s) => s.bytes)
    const statuses = [...new Set(r.samples.map((s) => s.status))].join(',')
    console.log(
      `| ${r.op.method} | \`${r.op.path}\` | ${statuses} | ${fmt(pct(ms, 50))} | ${fmt(pct(ms, 95))} | ${fmt(Math.max(...ms))} | ${fmt(pct(by, 50))} |`,
    )
  }

  // ----- skipped breakdown -----
  const skipped = results.filter((r) => r.skipped)
  const byReason = new Map<string, Result[]>()
  for (const r of skipped) {
    const k = r.skipped ?? '?'
    byReason.set(k, [...(byReason.get(k) ?? []), r])
  }
  console.log(`\n## skipped (${skipped.length})\n`)
  for (const [reason, rs] of [...byReason.entries()].sort((a, b) => b[1].length - a[1].length)) {
    console.log(`- **${reason}** (${rs.length}): ${rs.slice(0, 5).map((r) => `${r.op.method} ${r.op.path}`).join(', ')}${rs.length > 5 ? `, … +${rs.length - 5}` : ''}`)
  }

  // ----- mail-open critical path -----
  await probeMailOpen()
}

main().catch((e) => {
  console.error('bench failed:', e)
  process.exit(1)
})
