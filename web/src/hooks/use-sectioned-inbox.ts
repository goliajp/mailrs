// Whether the Inbox is shown by section, read from the server and saved
// there, so the phone apps follow the same choice.

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import {
  fetchSectionedInbox,
  saveSectionedInbox,
  sectionedInboxKey,
  sectionedOf,
} from '@/lib/sectioned-inbox'

export function useSectionedInbox(): boolean {
  const { data } = useQuery({
    queryKey: sectionedInboxKey,
    queryFn: ({ signal }) => fetchSectionedInbox(signal),
  })
  return sectionedOf(data)
}

export function useSetSectionedInbox() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (enabled: boolean) => saveSectionedInbox(enabled),
    onMutate: (enabled) => queryClient.setQueryData(sectionedInboxKey, { enabled }),
    onSettled: () => queryClient.invalidateQueries({ queryKey: sectionedInboxKey }),
  })
}
