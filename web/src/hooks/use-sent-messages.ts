import { useQuery } from '@tanstack/react-query'

import { mailKeys } from '@/lib/query-keys'
import { getToken } from '@/store/auth'
import { wireListSentMessages } from '@/wire/endpoints/mail'

// per-message Sent list (one row per outbound message, not per thread).
// both lanes serve GET /api/mail/sent with the same shape.
export function useSentMessagesQuery() {
  return useQuery({
    enabled: Boolean(getToken()),
    queryKey: mailKeys.sent(),
    staleTime: 30_000,
    queryFn: () => wireListSentMessages(),
  })
}
