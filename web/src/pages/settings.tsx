import { useAtomValue } from 'jotai'
import { lazy, Suspense, useId, useRef } from 'react'
import { useSearchParams } from 'react-router'

import { type Key, useT } from '@/i18n'
import { authAtom } from '@/store/auth'

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
const ForwardingSection = lazy(() =>
  import('@/components/settings/forwarding-section').then((m) => ({
    default: m.ForwardingSection,
  }))
)
const NotificationsSection = lazy(() =>
  import('@/components/settings/notifications-section').then((m) => ({
    default: m.NotificationsSection,
  }))
)
const ResetCodesSection = lazy(() =>
  import('@/components/settings/reset-codes-section').then((m) => ({
    default: m.ResetCodesSection,
  }))
)
const RulesSection = lazy(() =>
  import('@/components/settings/rules-section').then((m) => ({ default: m.RulesSection }))
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
const TemplatesSection = lazy(() =>
  import('@/components/settings/templates-section').then((m) => ({
    default: m.TemplatesSection,
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
  | 'forwarding'
  | 'keys'
  | 'notifications'
  | 'reset-codes'
  | 'rules'
  | 'security'
  | 'senders'
  | 'signatures'
  | 'templates'
  | 'webhooks'

type Group = 'account' | 'appearance' | 'developer' | 'mail'

const GROUPS: { key: Group; label: Key }[] = [
  { key: 'account', label: 'settings.group.account' },
  { key: 'mail', label: 'settings.group.mail' },
  { key: 'appearance', label: 'settings.group.appearance' },
  { key: 'developer', label: 'settings.group.developer' },
]

// in display order, group by group
const CATEGORIES: { group: Group; key: Category; label: Key }[] = [
  { group: 'account', key: 'account', label: 'settings.cat.account' },
  { group: 'account', key: 'security', label: 'settings.cat.security' },
  { group: 'account', key: 'keys', label: 'settings.cat.keys' },
  // issuers only
  { group: 'account', key: 'reset-codes', label: 'settings.cat.reset-codes' },
  { group: 'mail', key: 'signatures', label: 'settings.cat.signatures' },
  { group: 'mail', key: 'templates', label: 'settings.cat.templates' },
  { group: 'mail', key: 'rules', label: 'settings.cat.rules' },
  { group: 'mail', key: 'forwarding', label: 'settings.cat.forwarding' },
  { group: 'mail', key: 'senders', label: 'settings.cat.senders' },
  { group: 'mail', key: 'notifications', label: 'settings.cat.notifications' },
  { group: 'appearance', key: 'appearance', label: 'settings.cat.appearance' },
  { group: 'developer', key: 'api-keys', label: 'settings.cat.api-keys' },
  { group: 'developer', key: 'webhooks', label: 'settings.cat.webhooks' },
  { group: 'developer', key: 'calendar-feeds', label: 'settings.cat.calendar-feeds' },
]

const CATEGORY_KEYS = new Set(CATEGORIES.map((c) => c.key))

// Sections that render a data table get the full panel width — a 42rem
// column truncates columns that have the room to be read.
const WIDE_CATEGORIES = new Set<Category>(['api-keys'])

// Table state (search / sort / page) lives in the URL; it belongs to the
// section that put it there, so switching sections clears it.
const TABLE_PARAMS = ['dir', 'page', 'q', 'size', 'sort']

export function Settings() {
  const t = useT()
  const tabIds = useId()
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([])
  const [searchParams, setSearchParams] = useSearchParams()
  const active = parseTab(searchParams.get('tab'))
  // the server refuses a reset code to anyone else; the tab is only a convenience
  const issuer = useAtomValue(authAtom)?.is_reset_issuer === true
  const shown = CATEGORIES.filter((c) => c.key !== 'reset-codes' || issuer)

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
        <h1 className="text-lg font-semibold tracking-tight">{t('settings.title')}</h1>
      </header>

      <div className="flex min-h-0 flex-1 flex-col sm:flex-row">
        {/* sidebar - horizontal on mobile, vertical on desktop */}
        <nav
          aria-label={t('settings.sections')}
          className="border-border flex shrink-0 gap-1 overflow-x-auto border-b p-2 sm:w-48 sm:flex-col sm:overflow-x-visible sm:overflow-y-auto sm:border-r sm:border-b-0 sm:p-3"
          onKeyDown={(e) => {
            const from = shown.findIndex((c) => c.key === active)
            const to = nextTabIndex(e.key, from, shown.length)
            if (to === null) return
            e.preventDefault()
            setActive(shown[to].key)
            tabRefs.current[to]?.focus()
          }}
        >
          {GROUPS.map((group) => {
            const tabs = shown.filter((c) => c.group === group.key)
            if (tabs.length === 0) return null
            return (
              <div className="flex shrink-0 gap-1 sm:flex-col" key={group.key}>
                <p
                  className="text-fg-muted hidden px-3 pt-2 text-xs sm:block"
                  id={`${tabIds}-group-${group.key}`}
                >
                  {t(group.label)}
                </p>
                <div
                  aria-labelledby={`${tabIds}-group-${group.key}`}
                  aria-orientation="vertical"
                  className="flex gap-1 sm:flex-col"
                  role="tablist"
                >
                  {tabs.map((cat) => {
                    const i = shown.indexOf(cat)
                    return (
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
                        {t(cat.label)}
                      </button>
                    )
                  })}
                </div>
              </div>
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
          <div className={panelWidthClass(active)}>
            <Suspense fallback={<SectionFallback />}>
              {active === 'account' && <AccountSection />}
              {active === 'security' && <SecuritySection />}
              {active === 'signatures' && <SignaturesSection />}
              {active === 'templates' && <TemplatesSection />}
              {active === 'rules' && <RulesSection />}
              {active === 'forwarding' && <ForwardingSection />}
              {active === 'senders' && <SendersSection />}
              {active === 'notifications' && <NotificationsSection />}
              {active === 'keys' && <EncryptionKeysSection />}
              {active === 'api-keys' && <ApiKeysSection />}
              {active === 'webhooks' && <WebhooksSection />}
              {active === 'calendar-feeds' && <CalendarFeedsSection />}
              {active === 'appearance' && <AppearanceSection />}
              {active === 'reset-codes' && <ResetCodesSection />}
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
  const t = useT()
  return (
    <div className="text-fg-muted flex items-center gap-2 py-4 text-sm" role="status">
      <div className="h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent" />
      {t('common.loading')}
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
