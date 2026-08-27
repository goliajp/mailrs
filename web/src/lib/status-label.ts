/**
 * A status dot, and the word beside it.
 *
 * The rule is written down in `components/status-bar.tsx` — "Users
 * can't remember which colour dot means backend vs realtime", from a
 * 2026-07-07 bug report — and two places had never got it: the admin
 * accounts table showed active/inactive as green against grey at 8×8
 * with a `title` on a span nobody can focus, and the service pills
 * showed up/down as green against red, which is the pair about 8% of
 * men cannot separate, on the question you can least afford to guess.
 *
 * Here rather than in each page so both read one definition, and so
 * the words can be tested without a router and an admin session.
 */

/// Whether an account is in use.
export function activeLabel(active: boolean): string {
  if (active) return 'Active'
  return 'Inactive'
}

/// Whether a service is answering.
export function serviceLabel(ok: boolean): string {
  if (ok) return 'up'
  return 'down'
}

/// The dot for a thing that is on or off.
export function stateDotClass(ok: boolean, offColour: string): string {
  const base = 'inline-block h-2 w-2 shrink-0 rounded-full'
  if (ok) return `${base} bg-success`
  return `${base} ${offColour}`
}
