import { useAtomValue } from 'jotai'
import { useCallback, useEffect, useState } from 'react'
import { Navigate, Route, Routes, useLocation } from 'react-router'

import { AppSidebar } from '@/components/app-sidebar'
import { CommandPalette } from '@/components/command-palette'
import { ErrorBoundary } from '@/components/error-boundary'
import { Admin } from '@/pages/admin'
import { Chat } from '@/pages/chat'
import { Login } from '@/pages/login'
import { ResetPassword } from '@/pages/reset-password'
import { Playground } from '@/pages/playground'
import { Protocol } from '@/pages/protocol'
import { Settings } from '@/pages/settings'
import { authAtom } from '@/store/auth'
import { unreadCountAtom } from '@/store/chat'

function RequireAuth({ children }: { children: React.ReactNode }) {
  const auth = useAtomValue(authAtom)
  if (!auth) return <Navigate to="/login" replace />
  return children
}

function StatusBar() {
  const auth = useAtomValue(authAtom)
  const location = useLocation()
  const [health, setHealth] = useState<{ status: string; version: string; pg: boolean; valkey: boolean } | null>(null)

  const fetchHealth = useCallback(async () => {
    try {
      const res = await fetch('/api/health')
      if (res.ok) setHealth(await res.json())
    } catch { /* ignore */ }
  }, [])

  useEffect(() => {
    fetchHealth()
    const id = setInterval(fetchHealth, 30000)
    return () => clearInterval(id)
  }, [fetchHealth])

  const section = location.pathname.startsWith('/admin') ? 'Admin'
    : location.pathname.startsWith('/protocol') ? 'Monitor'
    : location.pathname.startsWith('/settings') ? 'Settings'
    : 'Mail'

  return (
    <div className="flex h-7 shrink-0 items-center justify-between px-3 text-[11px] text-[var(--color-text-tertiary)]">
      <div className="flex items-center gap-2">
        {health && (
          <span className="flex items-center gap-1">
            <span className={`inline-block h-2 w-2 rounded-full ${health.status === 'healthy' ? 'bg-[var(--color-status-success)]' : health.status === 'degraded' ? 'bg-[var(--color-status-warning)]' : 'bg-[var(--color-status-danger)]'}`} />
            {health.pg ? 'PG' : ''}{health.pg && health.valkey ? ' · ' : ''}{health.valkey ? 'Valkey' : ''}
          </span>
        )}
        <span className="text-[var(--color-border-strong)]">·</span>
        <span>{section}</span>
      </div>
      <div className="flex items-center gap-2">
        {auth && <span>{auth.address}</span>}
        {auth && health && <span className="text-[var(--color-border-strong)]">·</span>}
        {health && <span>v{health.version}</span>}
      </div>
    </div>
  )
}

function AuthLayout({ children, raw }: { children: React.ReactNode; raw?: boolean }) {
  return (
    <RequireAuth>
      <div className="fixed inset-0 flex flex-col bg-[var(--color-bg-base)] text-[var(--color-text-primary)]">
        <div className="flex min-h-0 flex-1 gap-1 p-1">
          <AppSidebar />
          {raw ? children : (
            <div className="min-w-0 flex-1 overflow-hidden rounded-lg bg-[var(--color-bg-raised)]">
              {children}
            </div>
          )}
        </div>
        <StatusBar />
      </div>
    </RequireAuth>
  )
}

function useDocumentTitle() {
  const unreadCount = useAtomValue(unreadCountAtom)

  useEffect(() => {
    document.title = unreadCount > 0 ? `(${unreadCount}) Mailrs` : 'Mailrs'
  }, [unreadCount])
}

export function App() {
  useDocumentTitle()

  return (
    <ErrorBoundary>
      <CommandPalette />
      <Routes>
        <Route path="/login" element={<Login />} />
        <Route path="/reset-password" element={<ResetPassword />} />
        <Route path="/playground" element={<Playground />} />
        <Route
          path="/protocol"
          element={
            <AuthLayout>
              <Protocol />
            </AuthLayout>
          }
        />
        <Route
          path="/admin/*"
          element={
            <AuthLayout>
              <Admin />
            </AuthLayout>
          }
        />
        <Route
          path="/settings"
          element={
            <AuthLayout>
              <Settings />
            </AuthLayout>
          }
        />
        <Route path="/mail/*" element={<Navigate to="/" replace />} />
        <Route
          path="/*"
          element={
            <AuthLayout raw>
              <Chat />
            </AuthLayout>
          }
        />
      </Routes>
    </ErrorBoundary>
  )
}
