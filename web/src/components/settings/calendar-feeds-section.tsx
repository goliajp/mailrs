import type { CalendarFeed } from './_shared'

import { useQuery } from '@tanstack/react-query'
import { useEffect, useState } from 'react'

import { deleteJson, fetchList, postJson } from '@/lib/api'
import { queryClient } from '@/lib/query-client'
import { settingsKeys } from '@/lib/query-keys'

import { btnPrimary, inputClass, SectionHeader } from './_shared'

export function CalendarFeedsSection() {
  const feedsQuery = useQuery({
    queryKey: settingsKeys.calendarFeeds(),
    queryFn: async () => {
      const list = await fetchList<CalendarFeed>('/calendar/feeds')
      return list ?? []
    },
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
      setError(feedsQuery.error instanceof Error ? feedsQuery.error.message : 'failed to load')
    }
  }, [feedsQuery.error])

  const refresh = () => queryClient.invalidateQueries({ queryKey: settingsKeys.calendarFeeds() })

  const handleCreate = async () => {
    setError('')
    if (!url.trim()) {
      setError('URL is required')
      return
    }
    setCreating(true)
    try {
      await postJson('/calendar/feeds', {
        basic_auth_pass: authPass.trim() || null,
        basic_auth_user: authUser.trim() || null,
        name: name.trim(),
        url: url.trim(),
      })
      setUrl('')
      setName('')
      setAuthUser('')
      setAuthPass('')
      await refresh()
    } catch (e) {
      setError(e instanceof Error ? e.message : 'failed')
    } finally {
      setCreating(false)
    }
  }

  const handleDelete = async (id: number) => {
    if (!window.confirm('Remove this feed and all its synced events?')) return
    try {
      await deleteJson(`/calendar/feeds/${id}`)
      await refresh()
    } catch (e) {
      setError(e instanceof Error ? e.message : 'failed')
    }
  }

  return (
    <div className="space-y-6">
      <SectionHeader title="External Calendar Feeds" />
      <p className="text-fg-muted text-sm">
        Subscribe to a remote .ics URL (room calendar, public team calendar, Google Calendar export,
        Nextcloud / Radicale published calendar). The events appear alongside your own calendar.
        mailrs polls each feed at its refresh interval; macOS Calendar.app, Thunderbird, etc. pick
        the events up via mailrs's CalDAV server.
      </p>

      <div className="border-border space-y-3 rounded-md border p-4">
        <div className="text-fg text-sm font-medium">Add a feed</div>
        <input
          aria-label="Feed URL"
          className={inputClass}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="https://example.com/calendar.ics"
          type="url"
          value={url}
        />
        <input
          aria-label="Display name"
          className={inputClass}
          onChange={(e) => setName(e.target.value)}
          placeholder="Display name (optional)"
          type="text"
          value={name}
        />
        <div className="flex flex-col gap-2 sm:flex-row">
          <input
            aria-label="Basic auth user"
            autoComplete="off"
            className={inputClass + ' flex-1'}
            onChange={(e) => setAuthUser(e.target.value)}
            placeholder="Basic auth user (optional)"
            type="text"
            value={authUser}
          />
          <input
            aria-label="Basic auth password"
            autoComplete="new-password"
            className={inputClass + ' flex-1'}
            onChange={(e) => setAuthPass(e.target.value)}
            placeholder="Basic auth password"
            type="password"
            value={authPass}
          />
        </div>
        {error && <div className="text-danger text-xs">{error}</div>}
        <button
          className={btnPrimary}
          disabled={creating || !url.trim()}
          onClick={() => void handleCreate()}
        >
          {creating ? 'Adding…' : 'Add feed'}
        </button>
      </div>

      <div>
        <div className="text-fg mb-2 text-sm font-medium">
          Subscriptions {loading && <span className="text-fg-muted ml-2 text-xs">loading…</span>}
        </div>
        {feeds.length === 0 && !loading ? (
          <div className="text-fg-muted text-sm">No feeds yet.</div>
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
                    Refresh every {Math.round(f.refresh_interval_secs / 60)}m
                    {f.last_synced_at && (
                      <>
                        {' · last synced '}
                        {new Date(f.last_synced_at).toLocaleString(undefined, {
                          dateStyle: 'short',
                          timeStyle: 'short',
                        })}
                      </>
                    )}
                    {!f.enabled && ' · disabled'}
                  </div>
                  {f.last_error && <div className="text-danger mt-1 text-xs">⚠ {f.last_error}</div>}
                </div>
                <button
                  className="text-fg-muted hover:text-fg shrink-0 text-xs underline-offset-2 hover:underline"
                  onClick={() => void handleDelete(f.id)}
                >
                  Remove
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  )
}
