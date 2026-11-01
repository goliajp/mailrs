import { useQuery } from '@tanstack/react-query'
import { useEffect, useState } from 'react'

import { currentLocale, type Key, useT } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { queryClient } from '@/lib/query-client'
import { settingsKeys } from '@/lib/query-keys'
import {
  wireCreateCalendarFeed,
  wireDeleteCalendarFeed,
  wireListCalendarFeeds,
} from '@/wire/endpoints/settings'

import { btnPrimary, inputClass, SectionHeader } from './_shared'

export function CalendarFeedsSection() {
  const t = useT()
  const feedsQuery = useQuery({
    queryKey: settingsKeys.calendarFeeds(),
    queryFn: () => wireListCalendarFeeds().then((items) => [...items]),
  })
  const feeds = feedsQuery.data ?? []
  const loading = feedsQuery.isFetching
  const [url, setUrl] = useState('')
  const [name, setName] = useState('')
  const [authUser, setAuthUser] = useState('')
  const [authPass, setAuthPass] = useState('')
  const [error, setError] = useState('')
  const [creating, setCreating] = useState(false)

  useEffect(() => {
    if (feedsQuery.error) {
      setError(errorText(feedsQuery.error, t('settings.feeds.loadFailed')))
    }
  }, [feedsQuery.error, t])

  const refresh = () => queryClient.invalidateQueries({ queryKey: settingsKeys.calendarFeeds() })

  const handleCreate = async () => {
    setError('')
    if (!url.trim()) {
      setError(t('settings.feeds.urlRequired'))
      return
    }
    setCreating(true)
    try {
      await wireCreateCalendarFeed({
        basicAuthPass: authPass || undefined,
        basicAuthUser: authUser.trim() || undefined,
        name: name.trim(),
        url: url.trim(),
      })
      setUrl('')
      setName('')
      setAuthUser('')
      setAuthPass('')
      await refresh()
    } catch (e) {
      setError(errorText(e, t('common.failed')))
    } finally {
      setCreating(false)
    }
  }

  const handleDelete = async (id: number) => {
    if (!window.confirm(t('settings.feeds.removeConfirm'))) return
    try {
      await wireDeleteCalendarFeed(id)
      await refresh()
    } catch (e) {
      setError(errorText(e, t('common.failed')))
    }
  }

  return (
    <div className="space-y-6">
      <SectionHeader title={t('settings.feeds.title')} />
      <p className="text-fg-muted text-sm">{t('settings.feeds.help')}</p>

      <div className="border-border space-y-3 rounded-md border p-4">
        <div className="text-fg text-sm font-medium">{t('settings.feeds.addTitle')}</div>
        <input
          aria-label={t('settings.feeds.url')}
          className={inputClass}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="https://example.com/calendar.ics"
          type="url"
          value={url}
        />
        <input
          aria-label={t('settings.feeds.displayName')}
          className={inputClass}
          onChange={(e) => setName(e.target.value)}
          placeholder={t('settings.feeds.displayNameOptional')}
          type="text"
          value={name}
        />
        {/* Restored 2026-07-31 with the fetcher that consumes them. They
            were removed the day before because the prod lane had no feed
            worker at all, so the password was discarded on arrival and
            storing it would have put a secret at rest with no consumer. */}
        <input
          aria-label={t('settings.feeds.authUser')}
          autoComplete="off"
          className={inputClass}
          onChange={(e) => setAuthUser(e.target.value)}
          placeholder={t('settings.feeds.authUserPlaceholder')}
          type="text"
          value={authUser}
        />
        <input
          aria-label={t('settings.feeds.authPass')}
          autoComplete="new-password"
          className={inputClass}
          onChange={(e) => setAuthPass(e.target.value)}
          placeholder={t('login.password')}
          type="password"
          value={authPass}
        />
        {error && <div className="text-danger text-xs">{error}</div>}
        <button
          className={btnPrimary}
          disabled={creating || !url.trim()}
          onClick={() => void handleCreate()}
        >
          {t(addLabel(creating))}
        </button>
      </div>

      <div>
        <div className="text-fg mb-2 text-sm font-medium">
          {t('settings.feeds.subscriptions')}{' '}
          {loading && (
            <span className="text-fg-muted ml-2 text-xs">{t('settings.feeds.loading')}</span>
          )}
        </div>
        {feeds.length === 0 && !loading ? (
          <div className="text-fg-muted text-sm">{t('settings.feeds.empty')}</div>
        ) : (
          <ul className="space-y-2">
            {feeds.map((f) => (
              <li
                className="border-border bg-bg-secondary flex items-start justify-between gap-3 rounded-md border p-3"
                key={f.id}
              >
                <div className="min-w-0 flex-1">
                  <div className="text-fg truncate text-sm font-medium">{f.name || f.url}</div>
                  <div className="text-fg-muted truncate text-xs">{f.url}</div>
                  <div className="text-fg-muted mt-1 text-xs">
                    {t('settings.feeds.refreshEvery', {
                      minutes: Math.round(f.refresh_interval_secs / 60),
                    })}
                    {syncText(f.last_synced_at, f.lastEventCount, t)}
                    {f.hasBasicAuth && t('settings.feeds.authenticated')}
                  </div>
                  {f.last_error && <div className="text-danger mt-1 text-xs">⚠ {f.last_error}</div>}
                </div>
                <button
                  className="text-fg-muted hover:text-fg shrink-0 text-xs underline-offset-2 hover:underline"
                  onClick={() => void handleDelete(f.id)}
                >
                  {t('settings.senders.remove')}
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  )
}

function addLabel(creating: boolean): Key {
  if (creating) return 'settings.feeds.adding'
  return 'settings.feeds.add'
}

function syncText(
  lastSyncedAt: null | number,
  count: number,
  t: (key: Key, vars?: Record<string, number | string>) => string
): string {
  if (lastSyncedAt === null) return t('settings.feeds.neverSynced')
  // epoch seconds from the backend; Date takes ms
  const at = new Date(lastSyncedAt * 1000).toLocaleString(currentLocale(), {
    dateStyle: 'short',
    timeStyle: 'short',
  })
  return t('settings.feeds.lastSynced', { at, count })
}
