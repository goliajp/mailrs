import { useAtomValue } from 'jotai'
import { useEffect } from 'react'
import { Navigate, Route, Routes } from 'react-router'

import { ErrorBoundary } from '@/components/error-boundary'
import { Admin } from '@/pages/admin'
import { Chat } from '@/pages/chat'
import { Login } from '@/pages/login'
import { Protocol } from '@/pages/protocol'
import { Settings } from '@/pages/settings'
import { authAtom } from '@/store/auth'
import { unreadCountAtom } from '@/store/chat'

function RequireAuth({ children }: { children: React.ReactNode }) {
  const auth = useAtomValue(authAtom)
  if (!auth) return <Navigate to="/login" replace />
  return children
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
      <Routes>
        <Route path="/login" element={<Login />} />
        <Route path="/protocol" element={<Protocol />} />
        <Route
          path="/admin/*"
          element={
            <RequireAuth>
              <Admin />
            </RequireAuth>
          }
        />
        <Route
          path="/settings"
          element={
            <RequireAuth>
              <Settings />
            </RequireAuth>
          }
        />
        <Route path="/mail/*" element={<Navigate to="/" replace />} />
        <Route
          path="/*"
          element={
            <RequireAuth>
              <Chat />
            </RequireAuth>
          }
        />
      </Routes>
    </ErrorBoundary>
  )
}
