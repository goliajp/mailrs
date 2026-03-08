import { Navigate, Route, Routes } from 'react-router'

import { AdminSidebar } from '@/components/admin-sidebar'
import { AdminAccounts } from '@/pages/admin-accounts'
import { AdminAliases } from '@/pages/admin-aliases'
import { AdminDomains } from '@/pages/admin-domains'
import { AdminOverview } from '@/pages/admin-overview'
import { AdminQueues } from '@/pages/admin-queues'

export function Admin() {
  return (
    <div className="flex h-screen flex-col bg-[var(--color-bg-base)] text-[var(--color-text-primary)] md:flex-row">
      <AdminSidebar />
      <div className="min-h-0 flex-1">
      <Routes>
        <Route path="overview" element={<AdminOverview />} />
        <Route path="domains" element={<AdminDomains />} />
        <Route path="accounts" element={<AdminAccounts />} />
        <Route path="aliases" element={<AdminAliases />} />
        <Route path="queues" element={<AdminQueues />} />
        <Route path="*" element={<Navigate to="overview" replace />} />
      </Routes>
      </div>
    </div>
  )
}
