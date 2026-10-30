import { useSetAtom } from 'jotai'
import { useEffect } from 'react'

import { authAtom } from '@/store/auth'
import { wireGetMe } from '@/wire/endpoints/auth'

/**
 * Reload the stored profile once per page load. Permissions and the admin
 * flag are decided by the server and can change after sign-in; a profile
 * kept from the sign-in alone would show entries the server now refuses,
 * or hide ones it now allows. A failure leaves the stored profile as it is:
 * an expired session is caught by the first real request's 401.
 */
export function useRefreshProfile(): void {
  const setAuth = useSetAtom(authAtom)
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
  }, [setAuth])
}
