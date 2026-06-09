import { lazy, Suspense } from 'react'
import { Navigate, Route, Routes } from 'react-router'

import { AdminSidebar } from '@/components/admin-sidebar'

// admin sub-pages are lazy: an admin landing on /admin/overview shouldn't
// pay the bytes for system-config + mail-audit + audit-log etc. each sub-
// page is independent (no cross-imports), so per-route chunking lets the
// browser fetch only what's actually rendered. lighthouse "JavaScript
// execution time" on the overview tab drops correspondingly.
const AdminAccounts = lazy(() =>
  import('@/pages/admin-accounts').then((m) => ({ default: m.AdminAccounts }))
)
const AdminAliases = lazy(() =>
  import('@/pages/admin-aliases').then((m) => ({ default: m.AdminAliases }))
)
const AdminApps = lazy(() => import('@/pages/admin-apps').then((m) => ({ default: m.AdminApps })))
const AdminAuditLog = lazy(() =>
  import('@/pages/admin-audit-log').then((m) => ({ default: m.AdminAuditLog }))
)
const AdminDomains = lazy(() =>
  import('@/pages/admin-domains').then((m) => ({ default: m.AdminDomains }))
)
const AdminEmailGroups = lazy(() =>
  import('@/pages/admin-email-groups').then((m) => ({ default: m.AdminEmailGroups }))
)
const AdminGreylist = lazy(() =>
  import('@/pages/admin-greylist').then((m) => ({ default: m.AdminGreylist }))
)
const AdminGroups = lazy(() =>
  import('@/pages/admin-groups').then((m) => ({ default: m.AdminGroups }))
)
const AdminMailAudit = lazy(() =>
  import('@/pages/admin-mail-audit').then((m) => ({ default: m.AdminMailAudit }))
)
const AdminOverview = lazy(() =>
  import('@/pages/admin-overview').then((m) => ({ default: m.AdminOverview }))
)
const AdminQueues = lazy(() =>
  import('@/pages/admin-queues').then((m) => ({ default: m.AdminQueues }))
)
const AdminSystemConfig = lazy(() =>
  import('@/pages/admin-system-config').then((m) => ({ default: m.AdminSystemConfig }))
)

export function Admin() {
  return (
    <div className="flex h-full flex-col md:flex-row">
      <AdminSidebar />
      <div className="min-h-0 flex-1 overflow-auto">
        <Suspense fallback={<PaneFallback />}>
          <Routes>
            <Route element={<AdminOverview />} path="overview" />
            <Route element={<AdminDomains />} path="domains" />
            <Route element={<AdminAccounts />} path="accounts" />
            <Route element={<AdminAliases />} path="aliases" />
            <Route element={<AdminGroups />} path="groups" />
            <Route element={<AdminEmailGroups />} path="email-groups" />
            <Route element={<AdminApps />} path="apps" />
            <Route element={<AdminQueues />} path="queues" />
            <Route element={<AdminGreylist />} path="greylist" />
            <Route element={<AdminAuditLog />} path="audit-log" />
            <Route element={<AdminMailAudit />} path="mail-audit" />
            <Route element={<AdminSystemConfig />} path="system-config" />
            <Route element={<Navigate replace to="overview" />} path="*" />
          </Routes>
        </Suspense>
      </div>
    </div>
  )
}

function PaneFallback() {
  return (
    <div className="flex h-full items-center justify-center p-8">
      <div className="border-border border-t-accent h-5 w-5 animate-spin rounded-full border-2" />
    </div>
  )
}
