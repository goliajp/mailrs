import { Navigate, Route, Routes } from 'react-router'

import { AdminSidebar } from '@/components/admin-sidebar'
import { AdminAccounts } from '@/pages/admin-accounts'
import { AdminDomains } from '@/pages/admin-domains'
import { AdminQueues } from '@/pages/admin-queues'

export function Admin() {
  return (
    <div className="flex h-screen bg-white text-zinc-900 dark:bg-zinc-950 dark:text-zinc-100">
      <AdminSidebar />
      <Routes>
        <Route path="domains" element={<AdminDomains />} />
        <Route path="accounts" element={<AdminAccounts />} />
        <Route path="queues" element={<AdminQueues />} />
        <Route path="*" element={<Navigate to="domains" replace />} />
      </Routes>
    </div>
  )
}
