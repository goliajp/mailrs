import { Navigate, Route, Routes } from 'react-router'

import { AdminSidebar } from '@/components/admin-sidebar'
import { AdminAccounts } from '@/pages/admin-accounts'
import { AdminAliases } from '@/pages/admin-aliases'
import { AdminApps } from '@/pages/admin-apps'
import { AdminAuditLog } from '@/pages/admin-audit-log'
import { AdminDomains } from '@/pages/admin-domains'
import { AdminEmailGroups } from '@/pages/admin-email-groups'
import { AdminGroups } from '@/pages/admin-groups'
import { AdminMailAudit } from '@/pages/admin-mail-audit'
import { AdminOverview } from '@/pages/admin-overview'
import { AdminQueues } from '@/pages/admin-queues'
import { AdminSystemConfig } from '@/pages/admin-system-config'

export function Admin() {
  return (
    <div className="flex h-full flex-col md:flex-row">
      <AdminSidebar />
      <div className="min-h-0 flex-1 overflow-auto">
        <Routes>
          <Route element={<AdminOverview />} path="overview" />
          <Route element={<AdminDomains />} path="domains" />
          <Route element={<AdminAccounts />} path="accounts" />
          <Route element={<AdminAliases />} path="aliases" />
          <Route element={<AdminGroups />} path="groups" />
          <Route element={<AdminEmailGroups />} path="email-groups" />
          <Route element={<AdminApps />} path="apps" />
          <Route element={<AdminQueues />} path="queues" />
          <Route element={<AdminAuditLog />} path="audit-log" />
          <Route element={<AdminMailAudit />} path="mail-audit" />
          <Route element={<AdminSystemConfig />} path="system-config" />
          <Route element={<Navigate replace to="overview" />} path="*" />
        </Routes>
      </div>
    </div>
  )
}
