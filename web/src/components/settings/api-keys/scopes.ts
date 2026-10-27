/**
 * Agent-key scope vocabulary.
 *
 * Backend: `crates/webapi/src/session/scopes.rs — agent_scopes_allow`.
 *   - `admin`         → full owner access (every route the account can reach)
 *   - empty list      → refused everywhere; only keys made before scopes were required have one
 *   - `mail:write`    → everything except `/api/admin/*`
 *   - `mail:read`     → GET/HEAD only, and never `/api/admin/*`
 *   - `mail:send`     → send, and list the sends this key made; nothing else, no MCP.
 *                       Made on the host (`mailrs-webapi agent-key create`), not offered here
 *   - `mock`          → only `/api/mock/*` and the `mock_*` MCP tools
 *   - `mock:{ns}`     → only that one mock namespace; cannot create or delete it
 *
 * `create_agent_key` accepts `{name, scopes}` and refuses an empty list. No
 * key can reach `/api/agent/keys`; keys are made from a signed-in session. It has
 * never accepted an expiry — the old "expires in days" input was silently
 * discarded by the server, so it is gone.
 */

export type ScopePreset = 'full' | 'mail:read' | 'mail:write' | 'mock'

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
  {
    description: 'Disposable test mailboxes only. Cannot read or send real mail.',
    label: 'Mock mailboxes',
    value: 'mock',
  },
]

/** Label + tone for the scope cell. Empty scopes are the loudest case. */
export function describeScopes(scopes: readonly string[]): { label: string; tone: ScopeTone } {
  if (scopes.length === 0) return { label: 'No scope · refused', tone: 'elevated' }
  if (scopes.includes('admin')) return { label: 'Full access · admin', tone: 'elevated' }
  return { label: scopes.join(' · '), tone: 'normal' }
}

export function presetToScopes(preset: ScopePreset): string[] {
  switch (preset) {
    case 'full':
      return ['admin']
    case 'mail:read':
      return ['mail:read']
    case 'mail:write':
      return ['mail:write']
    case 'mock':
      return ['mock']
  }
}
