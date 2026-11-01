import { type Key } from '@/i18n'
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
 *   - `masked_email`  → masked addresses only, REST and JMAP `MaskedEmail/*`; no mail, no send
 *   - `jmap`          → every JMAP route (mail, send, uploads, push) and nothing else
 *   - `mock`          → only `/api/mock/*` and the `mock_*` MCP tools
 *   - `mock:{ns}`     → only that one mock namespace; cannot create or delete it
 *
 * `create_agent_key` accepts `{name, scopes}` and refuses an empty list. No
 * key can reach `/api/agent/keys`; keys are made from a signed-in session. It has
 * never accepted an expiry — the old "expires in days" input was silently
 * discarded by the server, so it is gone.
 */

export type ScopePreset = 'full' | 'jmap' | 'mail:read' | 'mail:write' | 'masked' | 'mock'

export type ScopeTone = 'elevated' | 'normal'

export const SCOPE_PRESETS: { description: Key; label: Key; value: ScopePreset }[] = [
  {
    description: 'settings.scope.full.help',
    label: 'settings.scope.full',
    value: 'full',
  },
  {
    description: 'settings.scope.write.help',
    label: 'settings.scope.write',
    value: 'mail:write',
  },
  {
    description: 'settings.scope.read.help',
    label: 'settings.scope.read',
    value: 'mail:read',
  },
  {
    description: 'settings.scope.jmap.help',
    label: 'settings.scope.jmap',
    value: 'jmap',
  },
  {
    description: 'settings.scope.masked.help',
    label: 'settings.scope.masked',
    value: 'masked',
  },
  {
    description: 'settings.scope.mock.help',
    label: 'settings.scope.mock',
    value: 'mock',
  },
]

/** Label + tone for the scope cell. Empty scopes are the loudest case. */
export function describeScopes(
  scopes: readonly string[],
  t: (key: Key) => string
): { label: string; tone: ScopeTone } {
  if (scopes.length === 0) return { label: t('settings.scope.none'), tone: 'elevated' }
  if (scopes.includes('admin')) return { label: t('settings.scope.admin'), tone: 'elevated' }
  return { label: scopes.join(' · '), tone: 'normal' }
}

export function presetToScopes(preset: ScopePreset): string[] {
  switch (preset) {
    case 'full':
      return ['admin']
    case 'jmap':
      return ['jmap']
    case 'mail:read':
      return ['mail:read']
    case 'mail:write':
      return ['mail:write']
    case 'masked':
      return ['masked_email']
    case 'mock':
      return ['mock']
  }
}
