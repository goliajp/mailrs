import { useCallback, useEffect, useState } from 'react'
import { toast } from 'sonner'

import { deleteJson, fetchJson, postJson, putJson } from '@/lib/api'

interface ApiResult {
  success: boolean
}

interface AppInfo {
  active: boolean
  app_id: string
  created_at: string
  description: string
  id: number
  name: string
  owner_address: string
  scopes: string
}

interface CreateAppResponse {
  api_key: CreatedKey
  app_id: string
  name: string
  scopes: string
}

interface CreatedKey {
  id: string
  key: string
  prefix: string
}

export function AdminApps() {
  const [apps, setApps] = useState<AppInfo[]>([])
  const [adding, setAdding] = useState(false)
  const [permissions, setPermissions] = useState<string[]>([])
  const [form, setForm] = useState({ description: '', name: '' })
  const [selectedScopes, setSelectedScopes] = useState<Set<string>>(new Set())
  const [createdKey, setCreatedKey] = useState<CreatedKey | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<null | string>(null)
  const [expandedAppId, setExpandedAppId] = useState<null | string>(null)
  const [editScopes, setEditScopes] = useState<Set<string>>(new Set())
  const [savingScopes, setSavingScopes] = useState(false)

  const loadApps = useCallback(async () => {
    try {
      const data = await fetchJson<AppInfo[]>('/admin/apps')
      setApps(data)
    } catch {
      // keep current state on error
    }
  }, [])

  const loadPermissions = useCallback(async () => {
    try {
      const data = await fetchJson<string[]>('/admin/permissions')
      setPermissions(data)
    } catch {
      // keep current state on error
    }
  }, [])

  useEffect(() => {
    loadApps()
    loadPermissions()
  }, [loadApps, loadPermissions])

  const handleAdd = async () => {
    if (!form.name.trim()) return
    try {
      const result = await postJson<CreateAppResponse>('/admin/apps', {
        description: form.description.trim(),
        name: form.name.trim(),
        scopes: Array.from(selectedScopes).join(','),
      })
      toast.success(`App "${form.name.trim()}" created`)
      setCreatedKey(result.api_key)
      setForm({ description: '', name: '' })
      setSelectedScopes(new Set())
      setAdding(false)
      loadApps()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to create app')
    }
  }

  const handleDelete = async (appId: string) => {
    try {
      await deleteJson<ApiResult>(`/admin/apps/${appId}`)
      toast.success('App deleted')
      setDeleteTarget(null)
      loadApps()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to delete app')
      setDeleteTarget(null)
    }
  }

  const handleSaveScopes = async (appId: string) => {
    setSavingScopes(true)
    try {
      await putJson<ApiResult>(`/admin/apps/${appId}/scopes`, {
        scopes: Array.from(editScopes).join(','),
      })
      toast.success('Scopes updated')
      setExpandedAppId(null)
      loadApps()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to update scopes')
    } finally {
      setSavingScopes(false)
    }
  }

  const toggleScope = (
    scope: string,
    set: Set<string>,
    setter: (s: Set<string>) => void
  ) => {
    const next = new Set(set)
    if (next.has(scope)) {
      next.delete(scope)
    } else {
      next.add(scope)
    }
    setter(next)
  }

  const handleExpand = (app: AppInfo) => {
    if (expandedAppId === app.app_id) {
      setExpandedAppId(null)
      return
    }
    setExpandedAppId(app.app_id)
    const currentScopes = app.scopes
      ? app.scopes.split(',').filter(Boolean)
      : []
    setEditScopes(new Set(currentScopes))
  }

  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text).then(() => {
      toast.success('Copied to clipboard')
    })
  }

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <div className="mb-6 flex items-center justify-between">
        <h2 className="text-lg font-semibold">Apps</h2>
        <button
          className="rounded-md bg-[var(--color-bg-inverted)] px-3 py-1.5 text-sm font-medium text-[var(--color-text-on-inverted)] transition-colors hover:opacity-90"
          onClick={() => setAdding(true)}
        >
          Add App
        </button>
      </div>

      {createdKey && (
        <div className="mb-4 rounded-lg border border-[var(--color-status-warning)] bg-[var(--color-status-warning-subtle)] p-4">
          <p className="mb-2 text-sm font-semibold">API Key Created</p>
          <p className="mb-2 text-xs text-[var(--color-text-secondary)]">
            Copy this key now. It will not be shown again.
          </p>
          <div className="flex items-center gap-2">
            <code className="flex-1 rounded bg-[var(--color-bg-sunken)] px-3 py-1.5 font-mono text-sm">
              {createdKey.key}
            </code>
            <button
              className="rounded-md bg-[var(--color-bg-inverted)] px-3 py-1.5 text-sm font-medium text-[var(--color-text-on-inverted)] transition-colors hover:opacity-90"
              onClick={() => copyToClipboard(createdKey.key)}
            >
              Copy
            </button>
          </div>
          <button
            className="mt-2 text-xs text-[var(--color-text-secondary)] transition-colors hover:opacity-70"
            onClick={() => setCreatedKey(null)}
          >
            Dismiss
          </button>
        </div>
      )}

      {adding && (
        <div className="mb-4 space-y-2 rounded-lg border border-[var(--color-border-default)] p-4">
          <div className="flex gap-2">
            <input
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              placeholder="App name"
              value={form.name}
            />
            <input
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
              onChange={(e) =>
                setForm({ ...form, description: e.target.value })
              }
              placeholder="Description"
              value={form.description}
            />
          </div>
          {permissions.length > 0 && (
            <div className="flex flex-wrap gap-2">
              {permissions.map((perm) => (
                <label className="flex items-center gap-1.5 text-sm" key={perm}>
                  <input
                    checked={selectedScopes.has(perm)}
                    onChange={() =>
                      toggleScope(perm, selectedScopes, setSelectedScopes)
                    }
                    type="checkbox"
                  />
                  {perm}
                </label>
              ))}
            </div>
          )}
          <div className="flex gap-2">
            <button
              className="rounded-md bg-[var(--color-bg-inverted)] px-3 py-1.5 text-sm text-[var(--color-text-on-inverted)]"
              onClick={handleAdd}
            >
              Save
            </button>
            <button
              className="rounded-md px-3 py-1.5 text-sm text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)]"
              onClick={() => setAdding(false)}
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      <div className="overflow-hidden rounded-lg border border-[var(--color-border-default)]">
        <table className="w-full text-left text-sm">
          <thead className="border-b border-[var(--color-border-default)] bg-[var(--color-bg-sunken)]">
            <tr>
              <th className="px-4 py-2.5 font-medium">Name</th>
              <th className="px-4 py-2.5 font-medium">Scopes</th>
              <th className="px-4 py-2.5 font-medium">Owner</th>
              <th className="px-4 py-2.5 font-medium">App ID</th>
              <th className="px-4 py-2.5 text-right font-medium">Actions</th>
            </tr>
          </thead>
          <tbody>
            {apps.map((app) => (
              <>
                <tr
                  className="border-b border-[var(--color-border-default)] last:border-0"
                  key={app.app_id}
                >
                  <td className="px-4 py-3 font-medium">{app.name}</td>
                  <td className="px-4 py-3">
                    <div className="flex flex-wrap gap-1">
                      {app.scopes
                        ? app.scopes
                            .split(',')
                            .filter(Boolean)
                            .map((scope) => (
                              <span
                                className={
                                  scope === 'internal.rpc'
                                    ? 'inline-block rounded bg-[var(--color-status-success-subtle)] px-2 py-0.5 text-xs font-medium text-[var(--color-status-success)]'
                                    : 'inline-block rounded bg-[var(--color-bg-raised)] px-2 py-0.5 text-xs font-medium text-[var(--color-text-secondary)]'
                                }
                                key={scope}
                              >
                                {scope}
                              </span>
                            ))
                        : null}
                    </div>
                  </td>
                  <td className="px-4 py-3 text-[var(--color-text-secondary)]">
                    {app.owner_address}
                  </td>
                  <td className="px-4 py-3">
                    <span
                      className="max-w-[120px] truncate font-mono text-xs text-[var(--color-text-tertiary)]"
                      title={app.app_id}
                    >
                      {app.app_id}
                    </span>
                  </td>
                  <td className="px-4 py-3 text-right">
                    <div className="flex items-center justify-end gap-3">
                      <button
                        className="text-xs text-[var(--color-brand-primary)] transition-colors hover:opacity-70"
                        onClick={() => handleExpand(app)}
                      >
                        {expandedAppId === app.app_id ? 'Collapse' : 'Scopes'}
                      </button>
                      <button
                        className="text-xs text-[var(--color-status-danger)] transition-colors hover:opacity-70"
                        onClick={() => setDeleteTarget(app.app_id)}
                      >
                        Delete
                      </button>
                    </div>
                  </td>
                </tr>
                {expandedAppId === app.app_id && (
                  <tr
                    className="border-b border-[var(--color-border-default)] last:border-0"
                    key={`${app.app_id}-edit`}
                  >
                    <td className="px-4 py-3" colSpan={5}>
                      <div className="space-y-2 rounded-lg border border-[var(--color-border-default)] p-3">
                        <p className="text-xs font-medium text-[var(--color-text-secondary)]">
                          Edit Scopes
                        </p>
                        <div className="flex flex-wrap gap-2">
                          {permissions.map((perm) => (
                            <label
                              className="flex items-center gap-1.5 text-sm"
                              key={perm}
                            >
                              <input
                                checked={editScopes.has(perm)}
                                onChange={() =>
                                  toggleScope(perm, editScopes, setEditScopes)
                                }
                                type="checkbox"
                              />
                              {perm}
                            </label>
                          ))}
                        </div>
                        <div className="flex gap-2">
                          <button
                            className="rounded-md bg-[var(--color-bg-inverted)] px-3 py-1.5 text-sm text-[var(--color-text-on-inverted)] disabled:opacity-50"
                            disabled={savingScopes}
                            onClick={() => handleSaveScopes(app.app_id)}
                          >
                            {savingScopes ? 'Saving...' : 'Save'}
                          </button>
                          <button
                            className="rounded-md px-3 py-1.5 text-sm text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)]"
                            onClick={() => setExpandedAppId(null)}
                          >
                            Cancel
                          </button>
                        </div>
                      </div>
                    </td>
                  </tr>
                )}
              </>
            ))}
            {apps.length === 0 && (
              <tr>
                <td
                  className="px-4 py-8 text-center text-[var(--color-text-tertiary)]"
                  colSpan={5}
                >
                  No apps configured
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {deleteTarget && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
          onClick={() => setDeleteTarget(null)}
        >
          <div
            className="w-full max-w-sm rounded-lg bg-[var(--color-bg-raised)] p-6 shadow-lg"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="mb-2 text-sm font-semibold">Confirm Deletion</h3>
            <p className="mb-4 text-sm text-[var(--color-text-tertiary)]">
              Are you sure you want to delete this app? This action cannot be
              undone.
            </p>
            <div className="flex justify-end gap-2">
              <button
                className="rounded-md px-3 py-1.5 text-sm text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)]"
                onClick={() => setDeleteTarget(null)}
              >
                Cancel
              </button>
              <button
                className="rounded-md bg-[var(--color-status-danger)] px-3 py-1.5 text-sm font-medium text-white transition-colors hover:opacity-90"
                onClick={() => handleDelete(deleteTarget)}
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
