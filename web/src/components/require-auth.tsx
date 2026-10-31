import type React from 'react'

import { useAtomValue } from 'jotai'
import { Navigate } from 'react-router'

import { LoadingFallback } from '@/components/loading-fallback'
import { useRefreshProfile } from '@/hooks/use-refresh-profile'
import { authAtom } from '@/store/auth'

export function RequireAuth({ children }: { children: React.ReactNode }) {
  const auth = useAtomValue(authAtom)
  const settled = useRefreshProfile()
  if (auth) return children
  // no stored profile is not yet "signed out": the cookie may say otherwise
  if (!settled) return <LoadingFallback />
  return <Navigate replace to="/login" />
}
