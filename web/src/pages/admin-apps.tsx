import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { Boxes } from 'lucide-react'
import { Fragment, useState } from 'react'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTableSkeleton,
} from '@/components/admin-page'
import { MobileModal } from '@/components/mobile-modal'
import { ScrollableTable } from '@/components/scrollable-table'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { adminKeys } from '@/lib/query-keys'
import {
  adminDelete,
  adminListGet,
  adminPost,
  adminPut,
  adminStringListGet,
} from '@/wire/endpoints/admin'

// Backend: crates/webapi/src/handlers/apps_keys/apps.rs — fn list_apps
type AppInfo = {
  app_id: string
  created_at: number
  id: number
  name: string
  scopes: string[]
}

// Backend: crates/webapi/src/handlers/apps_keys/apps.rs — fn create_app
type CreatedApp = {
  app_id: string
  id: number
  secret: string
}

const HEADERS = ['Name', 'Scopes', 'App ID', 'Created', 'Actions']
const EMPTY_APPS: AppInfo[] = []
const EMPTY_PERMISSIONS: string[] = []

const PRIMARY_BUTTON = 'bg-fg text-bg rounded-md px-3 py-1.5 text-sm disabled:opacity-50'
const CANCEL_BUTTON =
  'text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors'

export function AdminApps() {
  const apps = useQuery({
    queryKey: adminKeys.apps(),
    queryFn: ({ signal }) => adminListGet<AppInfo>('/admin/apps', signal),
  })
  const permissionsQuery = useQuery({
    queryKey: adminKeys.permissions(),
    queryFn: ({ signal }) => adminStringListGet('/admin/permissions', signal),
  })
  const permissions = permissionsQuery.data ?? EMPTY_PERMISSIONS

  const [adding, setAdding] = useState(false)
  const [name, setName] = useState('')
  const [selectedScopes, setSelectedScopes] = useState<string[]>([])
  const [created, setCreated] = useState<CreatedApp | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<null | string>(null)

  const addApp = useAdminMutation({
    invalidateKey: adminKeys.apps(),
    mutationFn: (vars: { name: string; scopes: string[] }) =>
      adminPost<CreatedApp>('/admin/apps', vars),
    successMsg: (vars) => `App "${vars.name}" created`,
  })

  const deleteApp = useAdminMutation({
    invalidateKey: adminKeys.apps(),
    successMsg: 'App deleted',
    mutationFn: (appId: string) => adminDelete(`/admin/apps/${appId}`),
  })

  const closeForm = () => {
    setName('')
    setSelectedScopes([])
    setAdding(false)
  }

  const handleAdd = () => {
    if (!name.trim()) return
    addApp.mutate(
      { name: name.trim(), scopes: selectedScopes },
      {
        onSuccess: (result) => {
          setCreated(result)
          closeForm()
        },
      }
    )
  }

  const renderBody = () => {
    if (apps.isPending) return <AdminTableSkeleton cols={5} headers={HEADERS} rows={4} />
    if (apps.error) return <AdminErrorState error={apps.error} onRetry={() => apps.refetch()} />
    const rows = apps.data ?? EMPTY_APPS
    if (rows.length === 0) {
      if (adding) return null
      return (
        <AdminEmptyState
          description="An app has a name, a scope list and a secret shown once when it is created"
          icon={<Boxes className="h-10 w-10" />}
          title="No apps configured"
        />
      )
    }
    return (
      <ScrollableTable>
        <table className="w-full text-left text-sm">
          <thead className="border-border bg-bg-secondary border-b">
            <tr>
              {HEADERS.map((h) => (
                <th className="px-4 py-2.5 font-medium" key={h}>
                  {h}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((app) => (
              <AppRow
                app={app}
                key={app.app_id}
                onDelete={() => setDeleteTarget(app.app_id)}
                permissions={permissions}
              />
            ))}
          </tbody>
        </table>
      </ScrollableTable>
    )
  }

  return (
    <AdminPageShell
      actions={
        !adding && (
          <button
            className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm font-medium transition-colors hover:opacity-90"
            onClick={() => setAdding(true)}
          >
            Add App
          </button>
        )
      }
      title="Apps"
    >
      {created && <CreatedSecret app={created} onDismiss={() => setCreated(null)} />}

      {adding && (
        <div className="border-border mb-4 space-y-2 rounded-lg border p-4">
          <input
            aria-label="App name"
            autoFocus
            className="border-border bg-bg-secondary w-full rounded-md border px-3 py-1.5 text-sm"
            onChange={(e) => setName(e.target.value)}
            placeholder="App name"
            value={name}
          />
          <ScopeChecklist
            onChange={setSelectedScopes}
            permissions={permissions}
            selected={selectedScopes}
          />
          <div className="flex gap-2">
            <button
              className={PRIMARY_BUTTON}
              disabled={!name.trim() || addApp.isPending}
              onClick={handleAdd}
            >
              {busyLabel(addApp.isPending, 'Save', 'Saving...')}
            </button>
            <button className={CANCEL_BUTTON} onClick={closeForm}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {renderBody()}

      {deleteTarget && (
        <MobileModal onClose={() => setDeleteTarget(null)} open>
          <div className="bg-surface w-full max-w-sm rounded-lg p-6 shadow-lg">
            <h3 className="mb-2 text-sm font-semibold">Confirm Deletion</h3>
            <p className="text-fg-muted mb-4 text-sm">Delete this app? This cannot be undone</p>
            <div className="flex justify-end gap-2">
              <button className={CANCEL_BUTTON} onClick={() => setDeleteTarget(null)}>
                Cancel
              </button>
              <button
                className="bg-danger rounded-md px-3 py-1.5 text-sm font-medium text-white transition-colors hover:opacity-90 disabled:opacity-50"
                disabled={deleteApp.isPending}
                onClick={() =>
                  deleteApp.mutate(deleteTarget, { onSettled: () => setDeleteTarget(null) })
                }
              >
                {busyLabel(deleteApp.isPending, 'Delete', 'Deleting...')}
              </button>
            </div>
          </div>
        </MobileModal>
      )}
    </AdminPageShell>
  )
}

function AppRow({
  app,
  onDelete,
  permissions,
}: {
  app: AppInfo
  onDelete: () => void
  permissions: string[]
}) {
  const [editing, setEditing] = useState<null | string[]>(null)
  const saveScopes = useAdminMutation({
    invalidateKey: adminKeys.apps(),
    successMsg: 'Scopes updated',
    mutationFn: (scopes: string[]) => adminPut(`/admin/apps/${app.app_id}/scopes`, { scopes }),
  })

  const toggleEditing = () => {
    if (editing) {
      setEditing(null)
      return
    }
    setEditing(app.scopes)
  }

  let toggleLabel = 'Scopes'
  if (editing) toggleLabel = 'Collapse'

  return (
    <Fragment>
      <tr className="border-border border-b last:border-0">
        <td className="px-4 py-3 font-medium">{app.name}</td>
        <td className="px-4 py-3">
          <div className="flex flex-wrap gap-1">
            {app.scopes.map((scope) => (
              <span
                className="bg-surface text-fg-secondary inline-block rounded px-2 py-0.5 text-xs font-medium"
                key={scope}
              >
                {scope}
              </span>
            ))}
          </div>
        </td>
        <td className="text-fg-muted px-4 py-3 font-mono text-xs">{app.app_id}</td>
        <td className="text-fg-secondary px-4 py-3 text-xs">
          {new Date(app.created_at * 1000).toLocaleDateString()}
        </td>
        <td className="px-4 py-3 text-right">
          <div className="flex items-center justify-end gap-3">
            <button
              className="text-accent text-xs transition-colors hover:opacity-70"
              onClick={toggleEditing}
            >
              {toggleLabel}
            </button>
            <button
              className="text-danger text-xs transition-colors hover:opacity-70"
              onClick={onDelete}
            >
              Delete
            </button>
          </div>
        </td>
      </tr>
      {editing && (
        <tr className="border-border border-b last:border-0">
          <td className="px-4 py-3" colSpan={HEADERS.length}>
            <div className="border-border space-y-2 rounded-lg border p-3">
              <p className="text-fg-secondary text-xs font-medium">Edit Scopes</p>
              <ScopeChecklist onChange={setEditing} permissions={permissions} selected={editing} />
              <div className="flex gap-2">
                <button
                  className={PRIMARY_BUTTON}
                  disabled={saveScopes.isPending}
                  onClick={() => saveScopes.mutate(editing, { onSuccess: () => setEditing(null) })}
                >
                  {busyLabel(saveScopes.isPending, 'Save', 'Saving...')}
                </button>
                <button className={CANCEL_BUTTON} onClick={() => setEditing(null)}>
                  Cancel
                </button>
              </div>
            </div>
          </td>
        </tr>
      )}
    </Fragment>
  )
}

function busyLabel(busy: boolean, idle: string, working: string): string {
  if (busy) return working
  return idle
}

function CreatedSecret({ app, onDismiss }: { app: CreatedApp; onDismiss: () => void }) {
  const copy = () => {
    navigator.clipboard.writeText(app.secret).then(() => toast.success('Copied to clipboard'))
  }
  return (
    <div className="border-warning bg-warning/10 mb-4 rounded-lg border p-4" role="status">
      <p className="mb-2 text-sm font-semibold">Secret for {app.app_id}</p>
      <p className="text-fg-secondary mb-2 text-xs">
        Shown once. Only its hash is stored, so copy it now
      </p>
      <div className="flex items-center gap-2">
        <code className="bg-bg-secondary flex-1 rounded px-3 py-1.5 font-mono text-sm break-all">
          {app.secret}
        </code>
        <button className={PRIMARY_BUTTON} onClick={copy}>
          Copy
        </button>
      </div>
      <button
        className="text-fg-secondary mt-2 text-xs transition-colors hover:opacity-70"
        onClick={onDismiss}
      >
        Dismiss
      </button>
    </div>
  )
}

function ScopeChecklist({
  onChange,
  permissions,
  selected,
}: {
  onChange: (next: string[]) => void
  permissions: string[]
  selected: string[]
}) {
  const toggle = (perm: string) => {
    if (selected.includes(perm)) {
      onChange(selected.filter((s) => s !== perm))
      return
    }
    onChange([...selected, perm])
  }
  return (
    <div className="flex flex-wrap gap-2">
      {permissions.map((perm) => (
        <label className="flex items-center gap-1.5 text-sm" key={perm}>
          <input checked={selected.includes(perm)} onChange={() => toggle(perm)} type="checkbox" />
          {perm}
        </label>
      ))}
    </div>
  )
}
