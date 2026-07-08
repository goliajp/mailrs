import type { QueryKey, UseMutationOptions } from '@tanstack/react-query'

import { toast } from '@goliapkg/gds'
import { useMutation } from '@tanstack/react-query'

import { queryClient } from '@/lib/query-client'

// Generic admin-mutation template. All admin pages share the same shape:
//   1. fire a POST/PUT/DELETE against `/admin/...`
//   2. on success, show a green toast and invalidate the list-query that
//      reads the same resource
//   3. on error, show a red toast with the server-provided message
//
// Wrapping this in one place gives us three things that hand-rolled
// `await adminPost(); invalidate(); toast(); catch { toast(); }` blocks
// in the page didn't: (a) `isPending` so buttons can render a busy
// state, (b) automatic dedupe of duplicate clicks, (c) a single place
// to add rollback / optimistic update if we want it later.

export function useAdminMutation<TVars, TResult = unknown>(opts: {
  errorMsg?: string
  invalidateKey: QueryKey | QueryKey[]
  mutationFn: (vars: TVars) => Promise<TResult>
  // Caller can still tack on their own callbacks (e.g. clear form,
  // close modal) without losing the toast / invalidate behaviour.
  options?: Omit<UseMutationOptions<TResult, Error, TVars>, 'mutationFn' | 'onError' | 'onSuccess'>
  successMsg: ((vars: TVars, result: TResult) => string) | string
}) {
  return useMutation<TResult, Error, TVars>({
    mutationFn: opts.mutationFn,
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : (opts.errorMsg ?? 'Operation failed'))
    },
    onSuccess: (result, vars) => {
      const msg =
        typeof opts.successMsg === 'function' ? opts.successMsg(vars, result) : opts.successMsg
      toast.success(msg)
      const keys = Array.isArray(opts.invalidateKey[0])
        ? (opts.invalidateKey as QueryKey[])
        : [opts.invalidateKey as QueryKey]
      for (const key of keys) {
        queryClient.invalidateQueries({ queryKey: key }).catch(() => {})
      }
    },
    ...opts.options,
  })
}
