import { useAtomValue } from 'jotai'
import { useEffect } from 'react'
import { Navigate, Route, Routes } from 'react-router'

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

function AuthLayout({ children }: { children: React.ReactNode }) {
  return (
    <RequireAuth>
      <div className="flex h-screen bg-[var(--color-bg-base)] text-[var(--color-text-primary)]">
        <AppSidebar />
        <div className="min-w-0 flex-1">{children}</div>
      </div>
    </RequireAuth>
  )
}

function useDocumentTitle() {
  const unreadCount = useAtomValue(unreadCountAtom)

  useEffect(() => {
    document.title = unreadCount > 0 ? `(${unreadCount}) mailrs` : 'mailrs'
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
            <AuthLayout>
              <Chat />
            </AuthLayout>
          }
        />
      </Routes>
    </ErrorBoundary>
  )
}
