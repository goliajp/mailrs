import { useSetAtom } from 'jotai'
import { useEffect, useState } from 'react'

import { authAtom } from '@/store/auth'
import { wireGetMe } from '@/wire/endpoints/auth'

/**
 * Reload the stored profile once per page load. Permissions and the admin
 * flag are decided by the server and can change after sign-in; a profile
 * kept from the sign-in alone would show entries the server now refuses,
 * or hide ones it now allows. A failure leaves the stored profile as it is:
 * an expired session is caught by the first real request's 401.
 *
 * Returns whether the answer is in. A browser can hold a live session
 * cookie and no stored profile (site data cleared, or signed in by another
 * route); it is signed in, and the caller waits for this before deciding
 * it is not.
 */
export function useRefreshProfile(): boolean {
  const setAuth = useSetAtom(authAtom)
  const [settled, setSettled] = useState(false)
  useEffect(() => {
    wireGetMe()
      .then((me) =>
        setAuth({
          accessible_domains: me.accessible_domains,
          address: me.address,
          display_name: me.display_name,
          is_admin: me.is_admin,
          is_reset_issuer: me.is_reset_issuer,
          is_super: me.is_super,
          permissions: me.permissions,
        })
      )
      .catch(() => {})
      .finally(() => setSettled(true))
  }, [setAuth])
  return settled
}
