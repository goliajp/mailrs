import { useQuery } from '@tanstack/react-query'

import { settingsKeys } from '@/lib/query-keys'
import { wireListSignatures } from '@/wire/endpoints/settings'

/**
 * The signature this account signs with, from the server.
 *
 * Settings → Signatures has always saved to `/api/mail/signatures`,
 * and the composer has always read a `localStorage` atom that **no UI
 * ever wrote**. So a signature set up in Settings was never used, and
 * the one the composer read was permanently empty: two systems, one
 * of them invisible, neither connected to the other.
 *
 * The default one, or the first if the server holds several and marks
 * none — picking nothing would mean signing with nothing while the
 * settings page shows a signature sitting right there.
 */
export function useDefaultSignature(): { html: string; text: string } {
  const { data } = useQuery({
    queryKey: settingsKeys.signatures(),
    queryFn: () => wireListSignatures().then((items) => [...items]),
  })
  const chosen = data?.find((s) => s.is_default) ?? data?.[0]
  return { html: chosen?.html_content ?? '', text: chosen?.text_content ?? '' }
}
