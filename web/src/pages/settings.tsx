import { lazy, Suspense, useId, useRef } from 'react'
import { useSearchParams } from 'react-router'

const AccountSection = lazy(() =>
  import('@/components/settings/account-section').then((m) => ({ default: m.AccountSection }))
)
const ApiKeysSection = lazy(() =>
  import('@/components/settings/api-keys-section').then((m) => ({ default: m.ApiKeysSection }))
)
const AppearanceSection = lazy(() =>
  import('@/components/settings/appearance-section').then((m) => ({
    default: m.AppearanceSection,
  }))
)
const CalendarFeedsSection = lazy(() =>
  import('@/components/settings/calendar-feeds-section').then((m) => ({
    default: m.CalendarFeedsSection,
  }))
)
const EncryptionKeysSection = lazy(() =>
  import('@/components/settings/encryption-keys-section').then((m) => ({
    default: m.EncryptionKeysSection,
  }))
)
const SecuritySection = lazy(() =>
  import('@/components/settings/security-section').then((m) => ({ default: m.SecuritySection }))
)
const SendersSection = lazy(() =>
  import('@/components/settings/senders-section').then((m) => ({
    default: m.SendersSection,
  }))
)

const SignaturesSection = lazy(() =>
  import('@/components/settings/signatures-section').then((m) => ({
    default: m.SignaturesSection,
  }))
)
const WebhooksSection = lazy(() =>
  import('@/components/settings/webhooks-section').then((m) => ({ default: m.WebhooksSection }))
)

type Category =
  | 'account'
  | 'api-keys'
  | 'appearance'
  | 'calendar-feeds'
  | 'keys'
  | 'security'
  | 'senders'
  | 'signatures'
  | 'webhooks'

const CATEGORIES: { key: Category; label: string }[] = [
  { key: 'account', label: 'Account' },
  { key: 'security', label: 'Security' },
  { key: 'signatures', label: 'Signatures' },
  { key: 'senders', label: 'Senders' },
  { key: 'keys', label: 'Encryption Keys' },
  { key: 'api-keys', label: 'API Keys' },
  { key: 'webhooks', label: 'Webhooks' },
  { key: 'calendar-feeds', label: 'Calendar Feeds' },
  { key: 'appearance', label: 'Appearance' },
]

const CATEGORY_KEYS = new Set(CATEGORIES.map((c) => c.key))

// Sections that render a data table get the full panel width — a 42rem
// column truncates columns that have the room to be read.
const WIDE_CATEGORIES = new Set<Category>(['api-keys'])

// Table state (search / sort / page) lives in the URL; it belongs to the
// section that put it there, so switching sections clears it.
const TABLE_PARAMS = ['dir', 'page', 'q', 'size', 'sort']

export function Settings() {
  const tabIds = useId()
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([])
  const [searchParams, setSearchParams] = useSearchParams()
  const active = parseTab(searchParams.get('tab'))

  const setActive = (key: Category) => {
    setSearchParams((prev) => {
      const next = new URLSearchParams(prev)
      for (const name of TABLE_PARAMS) next.delete(name)
      if (key === 'account') next.delete('tab')
      else next.set('tab', key)
      return next
    })
  }

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <header className="border-border border-b px-4 py-3 sm:px-6">
        <h1 className="text-lg font-semibold tracking-tight">Settings</h1>
      </header>

      <div className="flex min-h-0 flex-1 flex-col sm:flex-row">
        {/* sidebar - horizontal on mobile, vertical on desktop */}
        <nav
          aria-label="Settings sections"
          aria-orientation="vertical"
          className="border-border flex shrink-0 gap-1 overflow-x-auto border-b p-2 sm:w-48 sm:flex-col sm:overflow-x-visible sm:overflow-y-auto sm:border-r sm:border-b-0 sm:p-3"
          onKeyDown={(e) => {
            const from = CATEGORIES.findIndex((c) => c.key === active)
            const to = nextTabIndex(e.key, from, CATEGORIES.length)
            if (to === null) return
            e.preventDefault()
            setActive(CATEGORIES[to].key)
            tabRefs.current[to]?.focus()
          }}
          role="tablist"
        >
          {CATEGORIES.map((cat, i) => (
            <button
              aria-controls={`${tabIds}-panel`}
              aria-selected={active === cat.key}
              className={tabClass(active === cat.key)}
              id={`${tabIds}-tab-${cat.key}`}
              key={cat.key}
              onClick={() => setActive(cat.key)}
              ref={(el) => {
                tabRefs.current[i] = el
              }}
              role="tab"
              tabIndex={active === cat.key ? 0 : -1}
              type="button"
            >
              {cat.label}
            </button>
          ))}
        </nav>

        {/* content panel */}
        <div
          aria-labelledby={`${tabIds}-tab-${active}`}
          className="min-h-0 flex-1 overflow-y-auto px-4 py-6 sm:px-8"
          id={`${tabIds}-panel`}
          role="tabpanel"
        >
          <div className={panelWidthClass(active)}>
            <Suspense fallback={<SectionFallback />}>
              {active === 'account' && <AccountSection />}
              {active === 'security' && <SecuritySection />}
              {active === 'signatures' && <SignaturesSection />}
              {active === 'senders' && <SendersSection />}
              {active === 'keys' && <EncryptionKeysSection />}
              {active === 'api-keys' && <ApiKeysSection />}
              {active === 'webhooks' && <WebhooksSection />}
              {active === 'calendar-feeds' && <CalendarFeedsSection />}
              {active === 'appearance' && <AppearanceSection />}
            </Suspense>
          </div>
        </div>
      </div>
    </div>
  )
}

/// Where an arrow key lands, or nothing when the key was not one.
///
/// The section list is a roving-tabindex tablist: eight of the nine
/// tabs carry `tabIndex={-1}`, which is correct **only** if arrow keys
/// move the roving point. There were none, so a keyboard user reached
/// the one active tab and could not get to the other eight — nine
/// settings screens behind a wall.
function nextTabIndex(key: string, from: number, count: number): null | number {
  switch (key) {
    case 'ArrowDown':
    case 'ArrowRight':
      return (from + 1) % count
    case 'ArrowLeft':
    case 'ArrowUp':
      return (from - 1 + count) % count
    case 'End':
      return count - 1
    case 'Home':
      return 0
    default:
      return null
  }
}

function panelWidthClass(active: Category): string {
  if (WIDE_CATEGORIES.has(active)) return 'w-full'
  return 'mx-auto max-w-2xl'
}

function parseTab(raw: null | string): Category {
  if (raw && CATEGORY_KEYS.has(raw as Category)) return raw as Category
  return 'account'
}

function SectionFallback() {
  return (
    <div className="text-fg-muted flex items-center gap-2 py-4 text-sm" role="status">
      <div className="h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent" />
      Loading...
    </div>
  )
}

/// A section tab, selected or not.
function tabClass(isActive: boolean): string {
  const base =
    'focus-visible:ring-accent/50 rounded-md px-3 py-1.5 text-left text-sm whitespace-nowrap transition-colors focus-visible:ring-2 focus-visible:outline-none'
  if (isActive) return `${base} bg-accent text-accent-fg`
  return `${base} text-fg-secondary hover:bg-bg-secondary`
}
