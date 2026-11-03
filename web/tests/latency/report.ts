/**
 * Turning per-iteration samples into the numbers the report prints, and
 * the machine-load witness that decides whether there is a report at all.
 */

import os from 'node:os'

export type Sample = {
  /** Requests started before the paint (the critical path), and their bytes. */
  critical: { bytes: number; requests: number }
  /** Event Timing of the input, ms: queueing delay and handler time. */
  event: null | { delay: number; handlers: number; presentation: number }
  /** Renderer main-thread time between input and paint, ms, by kind. */
  mainThread: { layout: number; other: number; script: number; style: number; total: number }
  ms: number
  /** Phases, ms: input → first API request, request → last response, → paint. */
  phases: { local: number; network: number; render: number; toRequest: number }
  /** Everything the interaction set off, until the network went quiet. */
  triggered: { bytes: number; requests: number; urls: string[] }
}

export type Summary = {
  bytes: { critical: number; triggered: number }
  mainThread: Sample['mainThread']
  n: number
  p50: number
  p95: number
  phases: Sample['phases']
  requests: { critical: number; triggered: number }
  timeouts: number
}

export function percentile(values: number[], p: number): number {
  const v = [...values].sort((a, b) => a - b)
  if (v.length === 0) return Number.NaN
  // nearest-rank: P95 of 30 samples is the 29th smallest
  const rank = Math.ceil((p / 100) * v.length)
  return v[Math.max(0, rank - 1)]
}

const median = (xs: number[]) => percentile(xs, 50)

/**
 * load1 sampled once a second for the whole run. load1 is itself a
 * one-minute average, so the first sample mostly describes the minute
 * before the run: that is the point of taking it.
 */
export class LoadWitness {
  private samples: number[] = []
  private timer: NodeJS.Timeout | null = null

  now() {
    return os.loadavg()[0]
  }

  start() {
    this.samples.push(os.loadavg()[0])
    this.timer = setInterval(() => this.samples.push(os.loadavg()[0]), 1000)
  }

  stop() {
    if (this.timer) clearInterval(this.timer)
    this.samples.push(os.loadavg()[0])
    return {
      end: this.samples[this.samples.length - 1],
      max: Math.max(...this.samples),
      samples: this.samples.length,
      start: this.samples[0],
    }
  }
}

export function summarize(samples: Sample[], timeouts: number): Summary {
  const pick = (f: (s: Sample) => number) => median(samples.map(f))
  return {
    bytes: { critical: pick((s) => s.critical.bytes), triggered: pick((s) => s.triggered.bytes) },
    mainThread: {
      layout: pick((s) => s.mainThread.layout),
      other: pick((s) => s.mainThread.other),
      script: pick((s) => s.mainThread.script),
      style: pick((s) => s.mainThread.style),
      total: pick((s) => s.mainThread.total),
    },
    n: samples.length,
    p50: percentile(
      samples.map((s) => s.ms),
      50
    ),
    p95: percentile(
      samples.map((s) => s.ms),
      95
    ),
    phases: {
      local: pick((s) => s.phases.local),
      network: pick((s) => s.phases.network),
      render: pick((s) => s.phases.render),
      toRequest: pick((s) => s.phases.toRequest),
    },
    requests: {
      critical: pick((s) => s.critical.requests),
      triggered: pick((s) => s.triggered.requests),
    },
    timeouts,
  }
}
