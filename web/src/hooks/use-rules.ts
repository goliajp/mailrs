// The signed-in user's rules. Running one changes conversations, so the mail
// lists are refetched after a run and after its undo.

import type { RuleBody } from '@/wire/schemas/rules'

import { useMutation, useQuery } from '@tanstack/react-query'

import { invalidateMail } from '@/lib/mail-cache'
import { queryClient } from '@/lib/query-client'
import {
  wireApplyRule,
  wireCreateRule,
  wireDeleteRule,
  wireListRules,
  wireUndoRuleRun,
  wireUpdateRule,
} from '@/wire/endpoints/rules'

export const rulesKey = ['rules'] as const

export function useApplyRule() {
  return useMutation({ onSettled: invalidateMail, mutationFn: (id: string) => wireApplyRule(id) })
}

export function useDeleteRule() {
  return useMutation({ onSettled: refresh, mutationFn: (id: string) => wireDeleteRule(id) })
}

export function useRules() {
  return useQuery({ queryKey: rulesKey, queryFn: ({ signal }) => wireListRules(signal) })
}

export function useSaveRule() {
  return useMutation({
    onSettled: refresh,
    mutationFn: ({ body, id }: { body: RuleBody; id?: string }) => {
      if (id) return wireUpdateRule(id, body)
      return wireCreateRule(body)
    },
  })
}

export function useUndoRuleRun() {
  return useMutation({
    onSettled: invalidateMail,
    mutationFn: (runId: string) => wireUndoRuleRun(runId),
  })
}

function refresh() {
  return queryClient.invalidateQueries({ queryKey: rulesKey })
}
