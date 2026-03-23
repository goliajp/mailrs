import { Navigate, Route, Routes } from 'react-router'

import { AdminSidebar } from '@/components/admin-sidebar'
import { AdminAccounts } from '@/pages/admin-accounts'
import { AdminAliases } from '@/pages/admin-aliases'
import { AdminApps } from '@/pages/admin-apps'
import { AdminAuditLog } from '@/pages/admin-audit-log'
import { AdminMailAudit } from '@/pages/admin-mail-audit'
import { AdminDomains } from '@/pages/admin-domains'
import { AdminEmailGroups } from '@/pages/admin-email-groups'
import { AdminGroups } from '@/pages/admin-groups'
import { AdminOverview } from '@/pages/admin-overview'
import { AdminQueues } from '@/pages/admin-queues'

export function Admin() {
  return (
    <div className="flex h-full flex-col md:flex-row">
      <AdminSidebar />
      <div className="min-h-0 flex-1 overflow-auto">
        <Routes>
          <Route path="overview" element={<AdminOverview />} />
          <Route path="domains" element={<AdminDomains />} />
          <Route path="accounts" element={<AdminAccounts />} />
          <Route path="aliases" element={<AdminAliases />} />
          <Route path="groups" element={<AdminGroups />} />
          <Route path="email-groups" element={<AdminEmailGroups />} />
          <Route path="apps" element={<AdminApps />} />
          <Route path="queues" element={<AdminQueues />} />
          <Route path="audit-log" element={<AdminAuditLog />} />
          <Route path="mail-audit" element={<AdminMailAudit />} />
          <Route path="*" element={<Navigate to="overview" replace />} />
        </Routes>
      </div>
    </div>
  )
}
