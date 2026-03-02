import { useAtomValue } from 'jotai'
import { Navigate, Route, Routes } from 'react-router'

import { Admin } from '@/pages/admin'
import { Chat } from '@/pages/chat'
import { Login } from '@/pages/login'
import { Protocol } from '@/pages/protocol'
import { authAtom } from '@/store/auth'

function RequireAuth({ children }: { children: React.ReactNode }) {
  const auth = useAtomValue(authAtom)
  if (!auth) return <Navigate to="/login" replace />
  return children
}

export function App() {
  return (
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
  )
}
