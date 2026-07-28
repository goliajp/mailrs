/**
 * Agent-key scope vocabulary.
 *
 * Backend: `crates/webapi/src/session.rs:80 — agent_scopes_allow`.
 *   - empty list      → full owner access (every route the account can reach)
 *   - `admin`         → same as empty
 *   - `mail:write`    → everything except `/api/admin/*`
 *   - `mail:read`     → GET/HEAD only, and never `/api/admin/*`
 *
 * `create_agent_key` (complete.rs:1320) accepts `{name, scopes}`. It has
 * never accepted an expiry — the old "expires in days" input was silently
 * discarded by the server, so it is gone.
 */

export type ScopePreset = 'full' | 'mail:read' | 'mail:write'

export type ScopeTone = 'elevated' | 'normal'

export const SCOPE_PRESETS: { description: string; label: string; value: ScopePreset }[] = [
  {
    description: 'Every route this account can reach, including admin APIs.',
    label: 'Full access',
    value: 'full',
  },
  {
    description: 'Read and write mail. Admin APIs are refused.',
    label: 'Mail read + write',
    value: 'mail:write',
  },
  {
    description: 'GET requests only. Cannot send, delete, or reach admin APIs.',
    label: 'Mail read-only',
    value: 'mail:read',
  },
]

/** Label + tone for the scope cell. Empty scopes are the loudest case. */
export function describeScopes(scopes: readonly string[]): { label: string; tone: ScopeTone } {
  if (scopes.length === 0) return { label: 'Full access', tone: 'elevated' }
  if (scopes.includes('admin')) return { label: 'Full access · admin', tone: 'elevated' }
  return { label: scopes.join(' · '), tone: 'normal' }
}

export function presetToScopes(preset: ScopePreset): string[] {
  switch (preset) {
    case 'full':
      return []
    case 'mail:read':
      return ['mail:read']
    case 'mail:write':
      return ['mail:write']
  }
}
