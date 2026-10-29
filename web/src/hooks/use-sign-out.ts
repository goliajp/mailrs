import { toast } from '@goliapkg/gds'
import { useSetAtom } from 'jotai'
import { useCallback } from 'react'
import { useNavigate } from 'react-router'

import { persister, queryClient } from '@/lib/query-client'
import { authAtom } from '@/store/auth'
import { wireLogout } from '@/wire/endpoints/auth'

/**
 * Sign out on the server, then forget this account in the browser.
 *
 * The session lives in an HttpOnly cookie the page cannot delete, so only
 * the server can end it. When that request fails the user is still signed
 * in and is told so, rather than shown a login page while the cookie keeps
 * working. The query cache is persisted to localStorage, so it is cleared
 * too: otherwise the subjects and previews of this mailbox stay on the
 * machine for the next person.
 */
export function useSignOut(): () => Promise<void> {
  const setAuth = useSetAtom(authAtom)
  const navigate = useNavigate()
  return useCallback(async () => {
    try {
      await wireLogout()
    } catch {
      toast.error('Could not sign out. Check your connection and try again')
      return
    }
    queryClient.clear()
    await persister.removeClient()
    setAuth(null)
    navigate('/login')
  }, [navigate, setAuth])
}
