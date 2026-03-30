import { toast } from '@goliapkg/gds'
import { useCallback, useEffect, useState } from 'react'

import { deleteJson, fetchJson, postJson, putJson } from '@/lib/api'

type ApiResult = {
  success: boolean
}

type AppInfo = {
  active: boolean
  app_id: string
  created_at: string
  description: string
  id: number
  name: string
  owner_address: string
  scopes: string
}

type CreateAppResponse = {
  api_key: CreatedKey
  app_id: string
  name: string
  scopes: string
}

type CreatedKey = {
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
          className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm font-medium transition-colors hover:opacity-90"
          onClick={() => setAdding(true)}
        >
          Add App
        </button>
      </div>

      {createdKey && (
        <div className="border-warning bg-warning/10 mb-4 rounded-lg border p-4">
          <p className="mb-2 text-sm font-semibold">API Key Created</p>
          <p className="text-fg-secondary mb-2 text-xs">
            Copy this key now. It will not be shown again.
          </p>
          <div className="flex items-center gap-2">
            <code className="bg-bg-secondary flex-1 rounded px-3 py-1.5 font-mono text-sm">
              {createdKey.key}
            </code>
            <button
              className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm font-medium transition-colors hover:opacity-90"
              onClick={() => copyToClipboard(createdKey.key)}
            >
              Copy
            </button>
          </div>
          <button
            className="text-fg-secondary mt-2 text-xs transition-colors hover:opacity-70"
            onClick={() => setCreatedKey(null)}
          >
            Dismiss
          </button>
        </div>
      )}

      {adding && (
        <div className="border-border mb-4 space-y-2 rounded-lg border p-4">
          <div className="flex gap-2">
            <input
              className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              placeholder="App name"
              value={form.name}
            />
            <input
              className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
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
              className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm"
              onClick={handleAdd}
            >
              Save
            </button>
            <button
              className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
              onClick={() => setAdding(false)}
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      <div className="border-border overflow-hidden rounded-lg border">
        <table className="w-full text-left text-sm">
          <thead className="border-border bg-bg-secondary border-b">
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
                  className="border-border border-b last:border-0"
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
                                    ? 'bg-success/10 text-success inline-block rounded px-2 py-0.5 text-xs font-medium'
                                    : 'bg-surface text-fg-secondary inline-block rounded px-2 py-0.5 text-xs font-medium'
                                }
                                key={scope}
                              >
                                {scope}
                              </span>
                            ))
                        : null}
                    </div>
                  </td>
                  <td className="text-fg-secondary px-4 py-3">
                    {app.owner_address}
                  </td>
                  <td className="px-4 py-3">
                    <span
                      className="text-fg-muted max-w-[120px] truncate font-mono text-xs"
                      title={app.app_id}
                    >
                      {app.app_id}
                    </span>
                  </td>
                  <td className="px-4 py-3 text-right">
                    <div className="flex items-center justify-end gap-3">
                      <button
                        className="text-accent text-xs transition-colors hover:opacity-70"
                        onClick={() => handleExpand(app)}
                      >
                        {expandedAppId === app.app_id ? 'Collapse' : 'Scopes'}
                      </button>
                      <button
                        className="text-danger text-xs transition-colors hover:opacity-70"
                        onClick={() => setDeleteTarget(app.app_id)}
                      >
                        Delete
                      </button>
                    </div>
                  </td>
                </tr>
                {expandedAppId === app.app_id && (
                  <tr
                    className="border-border border-b last:border-0"
                    key={`${app.app_id}-edit`}
                  >
                    <td className="px-4 py-3" colSpan={5}>
                      <div className="border-border space-y-2 rounded-lg border p-3">
                        <p className="text-fg-secondary text-xs font-medium">
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
                            className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm disabled:opacity-50"
                            disabled={savingScopes}
                            onClick={() => handleSaveScopes(app.app_id)}
                          >
                            {savingScopes ? 'Saving...' : 'Save'}
                          </button>
                          <button
                            className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
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
                <td className="text-fg-muted px-4 py-8 text-center" colSpan={5}>
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
            className="bg-surface w-full max-w-sm rounded-lg p-6 shadow-lg"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="mb-2 text-sm font-semibold">Confirm Deletion</h3>
            <p className="text-fg-muted mb-4 text-sm">
              Are you sure you want to delete this app? This action cannot be
              undone.
            </p>
            <div className="flex justify-end gap-2">
              <button
                className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
                onClick={() => setDeleteTarget(null)}
              >
                Cancel
              </button>
              <button
                className="bg-danger rounded-md px-3 py-1.5 text-sm font-medium text-white transition-colors hover:opacity-90"
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
