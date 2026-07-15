import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { deleteDraft, listDrafts, saveDraft, type SaveDraftRequest } from '@/lib/api'
import { mailKeys } from '@/lib/query-keys'
import { getToken } from '@/store/auth'

// server-backed drafts (both lanes serve /api/mail/drafts — fastcore over
// kevy, monolith over the PG `drafts` table). saveDraft upserts when the
// request carries an id, so a compose session updates one draft.

export function useDeleteDraftMutation() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: number) => deleteDraft(id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: mailKeys.drafts() })
    },
  })
}

export function useDraftsQuery() {
  return useQuery({
    enabled: Boolean(getToken()),
    queryFn: listDrafts,
    queryKey: mailKeys.drafts(),
    staleTime: 30_000,
  })
}

export function useSaveDraftMutation() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (req: SaveDraftRequest) => saveDraft(req),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: mailKeys.drafts() })
    },
  })
}
