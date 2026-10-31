// The signed-in user's saved searches. A setting, kept on the server so it
// follows the user to another browser.

import { useMutation, useQuery } from '@tanstack/react-query'

import { queryClient } from '@/lib/query-client'
import {
  wireCreateSavedSearch,
  wireDeleteSavedSearch,
  wireListSavedSearches,
} from '@/wire/endpoints/saved-searches'

export const savedSearchesKey = ['saved-searches'] as const

export function useCreateSavedSearch() {
  return useMutation({
    onSettled: refresh,
    mutationFn: ({ name, query }: { name: string; query: string }) =>
      wireCreateSavedSearch(name, query),
  })
}

export function useDeleteSavedSearch() {
  return useMutation({
    onSettled: refresh,
    mutationFn: (id: string) => wireDeleteSavedSearch(id),
  })
}

export function useSavedSearches() {
  return useQuery({
    queryKey: savedSearchesKey,
    queryFn: ({ signal }) => wireListSavedSearches(signal),
  })
}

function refresh() {
  return queryClient.invalidateQueries({ queryKey: savedSearchesKey })
}
