import { lazy, Suspense, useId } from 'react'
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
  | 'signatures'
  | 'webhooks'

const CATEGORIES: { key: Category; label: string }[] = [
  { key: 'account', label: 'Account' },
  { key: 'security', label: 'Security' },
  { key: 'signatures', label: 'Signatures' },
  { key: 'keys', label: 'Encryption Keys' },
  { key: 'api-keys', label: 'API Keys' },
  { key: 'webhooks', label: 'Webhooks' },
  { key: 'calendar-feeds', label: 'Calendar Feeds' },
  { key: 'appearance', label: 'Appearance' },
]

const CATEGORY_KEYS = new Set(CATEGORIES.map((c) => c.key))

export function Settings() {
  const tabIds = useId()
  const [searchParams, setSearchParams] = useSearchParams()
  const active = parseTab(searchParams.get('tab'))

  const setActive = (key: Category) => {
    setSearchParams((prev) => {
      const next = new URLSearchParams(prev)
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
          role="tablist"
        >
          {CATEGORIES.map((cat) => {
            const isActive = active === cat.key
            return (
              <button
                aria-controls={`${tabIds}-panel`}
                aria-selected={isActive}
                className={`rounded-md px-3 py-1.5 text-left text-sm whitespace-nowrap transition-colors ${
                  isActive ? 'bg-accent text-accent-fg' : 'text-fg-secondary hover:bg-bg-secondary'
                }`}
                id={`${tabIds}-tab-${cat.key}`}
                key={cat.key}
                onClick={() => setActive(cat.key)}
                role="tab"
                tabIndex={isActive ? 0 : -1}
                type="button"
              >
                {cat.label}
              </button>
            )
          })}
        </nav>

        {/* content panel */}
        <div
          aria-labelledby={`${tabIds}-tab-${active}`}
          className="min-h-0 flex-1 overflow-y-auto px-4 py-6 sm:px-8"
          id={`${tabIds}-panel`}
          role="tabpanel"
        >
          <div className="mx-auto max-w-2xl">
            <Suspense fallback={<SectionFallback />}>
              {active === 'account' && <AccountSection />}
              {active === 'security' && <SecuritySection />}
              {active === 'signatures' && <SignaturesSection />}
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
