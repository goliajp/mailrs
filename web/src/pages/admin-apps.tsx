import type { CreatedApp } from '@/components/admin-apps/created-secret'
import type { Column } from '@/components/admin-page'

import { useQuery } from '@tanstack/react-query'
import { Boxes } from 'lucide-react'
import { useState } from 'react'

import { AddAppForm, CANCEL_BUTTON, PRIMARY_BUTTON } from '@/components/admin-apps/add-app-form'
import { CreatedSecret } from '@/components/admin-apps/created-secret'
import { ScopeChecklist } from '@/components/admin-apps/scope-checklist'
import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTable,
  AdminTableSkeleton,
  ConfirmAction,
} from '@/components/admin-page'
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

type ScopeDraft = { appId: string; scopes: string[] }

const HEADERS = ['Name', 'Scopes', 'App ID', 'Created', '']
const EMPTY_APPS: AppInfo[] = []
const EMPTY_PERMISSIONS: string[] = []

// apps.rs: no request authenticates with an app secret, so the scopes gate nothing yet
const SUBTITLE =
  'An app is a name, a scope list and a secret shown once, stored for an integration to use. No request signs in with an app secret yet, so an app and its scopes grant no access'

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
  const [created, setCreated] = useState<CreatedApp | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<AppInfo | null>(null)
  const [editing, setEditing] = useState<null | ScopeDraft>(null)

  const addApp = useAdminMutation({
    invalidateKey: adminKeys.apps(),
    mutationFn: (vars: { name: string; scopes: string[] }) =>
      adminPost<CreatedApp>('/admin/apps', vars),
    successMsg: (vars) => `App "${vars.name}" added`,
  })

  const deleteApp = useAdminMutation({
    invalidateKey: adminKeys.apps(),
    successMsg: 'App deleted',
    mutationFn: (appId: string) => adminDelete(`/admin/apps/${appId}`),
  })

  const saveScopes = useAdminMutation({
    invalidateKey: adminKeys.apps(),
    successMsg: 'Scopes saved',
    mutationFn: (draft: ScopeDraft) =>
      adminPut(`/admin/apps/${draft.appId}/scopes`, { scopes: draft.scopes }),
  })

  const handleAdd = (vars: { name: string; scopes: string[] }) => {
    addApp.mutate(vars, {
      onSuccess: (result) => {
        setCreated(result)
        setAdding(false)
      },
    })
  }

  const toggleEditing = (app: AppInfo) => {
    if (editing?.appId === app.app_id) {
      setEditing(null)
      return
    }
    setEditing({ appId: app.app_id, scopes: app.scopes })
  }

  const scopeList = (selected: string[], onChange: (next: string[]) => void) => {
    if (permissionsQuery.error) {
      return (
        <AdminErrorState
          error={permissionsQuery.error}
          onRetry={() => permissionsQuery.refetch()}
          what="the scope list"
        />
      )
    }
    return <ScopeChecklist onChange={onChange} permissions={permissions} selected={selected} />
  }

  const columns: Column<AppInfo>[] = [
    { header: 'Name', key: 'name', cell: (a) => <span className="font-medium">{a.name}</span> },
    {
      header: 'Scopes',
      key: 'scopes',
      cell: (a) => (
        <div className="flex flex-wrap gap-1">
          {a.scopes.map((scope) => (
            <span
              className="bg-bg-secondary text-fg-secondary inline-block rounded px-2 py-0.5 text-xs font-medium"
              key={scope}
            >
              {scope}
            </span>
          ))}
        </div>
      ),
    },
    {
      header: 'App ID',
      key: 'app_id',
      cell: (a) => <span className="text-fg-muted font-mono text-xs">{a.app_id}</span>,
    },
    {
      align: 'end',
      header: 'Created',
      key: 'created',
      cell: (a) => (
        <span className="text-fg-secondary text-xs">
          {new Date(a.created_at * 1000).toLocaleDateString()}
        </span>
      ),
    },
    {
      align: 'end',
      header: 'Actions',
      hideHeader: true,
      key: 'actions',
      cell: (a) => (
        <div className="flex justify-end gap-3 text-xs">
          <button
            className="text-accent hover:opacity-70"
            onClick={() => toggleEditing(a)}
            type="button"
          >
            {editLabel(editing?.appId === a.app_id)}
          </button>
          <button
            className="text-danger hover:opacity-70"
            onClick={() => setDeleteTarget(a)}
            type="button"
          >
            Delete
          </button>
        </div>
      ),
    },
  ]

  const scopeEditor = (app: AppInfo) => {
    if (editing?.appId !== app.app_id) return null
    const draft = editing
    return (
      <div className="border-border space-y-3 rounded-lg border p-3">
        {scopeList(draft.scopes, (scopes) => setEditing({ ...draft, scopes }))}
        <div className="flex gap-2">
          <button
            className={PRIMARY_BUTTON}
            disabled={saveScopes.isPending}
            onClick={() => saveScopes.mutate(draft, { onSuccess: () => setEditing(null) })}
            type="button"
          >
            {busyLabel(saveScopes.isPending, 'Save scopes', 'Saving...')}
          </button>
          <button className={CANCEL_BUTTON} onClick={() => setEditing(null)} type="button">
            Cancel
          </button>
        </div>
      </div>
    )
  }

  const renderBody = () => {
    if (apps.isPending) return <AdminTableSkeleton cols={5} headers={HEADERS} rows={4} />
    if (apps.error) {
      return <AdminErrorState error={apps.error} onRetry={() => apps.refetch()} what="the apps" />
    }
    const rows = apps.data ?? EMPTY_APPS
    if (rows.length === 0) {
      if (adding) return null
      return (
        <AdminEmptyState
          description="Add an app to give a future integration a name, a scope list and a secret. Nothing checks app secrets yet, so adding one changes no one's access"
          icon={<Boxes className="h-10 w-10" />}
          title="No apps yet"
        />
      )
    }
    return (
      <AdminTable
        columns={columns}
        detail={scopeEditor}
        label="Apps"
        rowKey={(a) => a.app_id}
        rows={rows}
      />
    )
  }

  return (
    <AdminPageShell
      actions={
        !adding && (
          <button
            className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm font-medium transition-colors hover:opacity-90"
            onClick={() => setAdding(true)}
            type="button"
          >
            Add app
          </button>
        )
      }
      subtitle={SUBTITLE}
      title="Apps"
    >
      {created && <CreatedSecret app={created} onDismiss={() => setCreated(null)} />}

      {adding && (
        <AddAppForm
          onCancel={() => setAdding(false)}
          onSave={handleAdd}
          saving={addApp.isPending}
          scopeList={scopeList}
        />
      )}

      {renderBody()}

      {deleteTarget && (
        <ConfirmAction
          confirmLabel={busyLabel(deleteApp.isPending, 'Delete app', 'Deleting...')}
          consequence={`${deleteTarget.app_id}, its scopes and the hash of its secret are deleted, and the secret cannot be used again. No request signs in with an app secret yet, so nothing loses access`}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() =>
            deleteApp.mutate(deleteTarget.app_id, { onSettled: () => setDeleteTarget(null) })
          }
          pending={deleteApp.isPending}
          title={`Delete ${deleteTarget.name}?`}
        />
      )}
    </AdminPageShell>
  )
}

function busyLabel(busy: boolean, idle: string, working: string): string {
  if (busy) return working
  return idle
}

function editLabel(open: boolean): string {
  if (open) return 'Close scopes'
  return 'Edit scopes'
}
