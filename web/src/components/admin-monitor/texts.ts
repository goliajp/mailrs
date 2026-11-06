import type { Key, Lang } from '@/i18n'

/**
 * The daemon's text for a rule in the interface language; it carries all
 * three. Its sentences end in a full stop for the alert mail; on the page
 * the last one is dropped, as in every other line of the interface.
 */
export function inLang(lang: Lang, t: { en: string; ja: string; zh: string }): string {
  return withoutFinalStop(pick(lang, t))
}

export function stepOf(
  lang: Lang,
  a: { step_en: string; step_ja: string; step_zh: string }
): string {
  return inLang(lang, { en: a.step_en, ja: a.step_ja, zh: a.step_zh })
}

export function titleOf(
  lang: Lang,
  r: { title_en: string; title_ja: string; title_zh: string }
): string {
  return inLang(lang, { en: r.title_en, ja: r.title_ja, zh: r.title_zh })
}

function pick(lang: Lang, t: { en: string; ja: string; zh: string }): string {
  if (lang === 'zh') return t.zh
  if (lang === 'ja') return t.ja
  return t.en
}

function withoutFinalStop(s: string): string {
  return s.replace(/[.。]$/, '')
}

const PHASES: Record<string, Key> = {
  firing: 'admin.monitor.phase.firing',
  ok: 'admin.monitor.phase.ok',
  pending: 'admin.monitor.phase.pending',
  recovering: 'admin.monitor.phase.recovering',
  unknown: 'admin.monitor.phase.unknown',
}

/** The phase's name, or `null` for one this page does not know (shown as the daemon wrote it). */
export function phaseKey(phase: string): Key | null {
  return PHASES[phase] ?? null
}

export function severityKey(severity: string): Key {
  if (severity === 'critical') return 'admin.monitor.severity.critical'
  return 'admin.monitor.severity.warning'
}

/** The lengths a silence or maintenance can be set for: an end is required. */
export const DURATIONS: { key: Key; secs: number }[] = [
  { key: 'admin.monitor.dur.1h', secs: 3600 },
  { key: 'admin.monitor.dur.4h', secs: 4 * 3600 },
  { key: 'admin.monitor.dur.24h', secs: 24 * 3600 },
  { key: 'admin.monitor.dur.7d', secs: 7 * 86400 },
]

/**
 * The daemon's notification-log result for an own-path message the
 * sender's heartbeat listed (`ACCEPTED_250` in crates/ops/src/notify/route.rs).
 */
export const ACCEPTED_250 = "accepted by the receiving server (250), per the sender's heartbeat"
