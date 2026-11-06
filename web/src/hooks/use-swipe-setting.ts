// What a swipe on a row does, the user's setting on the server so it follows
// them to the phone apps. Until it has loaded, the default applies.

import type { SwipeSetting } from '@/lib/swipe'

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { DEFAULT_SWIPE } from '@/lib/swipe'
import { wireGetSwipe, wireSetSwipe } from '@/wire/endpoints/mail-settings'

export const swipeKey = ['settings', 'swipe'] as const

export function useSetSwipe() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (s: SwipeSetting) => wireSetSwipe(s),
    onMutate: (s) => queryClient.setQueryData(swipeKey, s),
    onSettled: () => queryClient.invalidateQueries({ queryKey: swipeKey }),
  })
}

export function useSwipeSetting(): SwipeSetting {
  const { data } = useQuery({
    queryKey: swipeKey,
    queryFn: ({ signal }) => wireGetSwipe(signal),
  })
  return data ?? DEFAULT_SWIPE
}
