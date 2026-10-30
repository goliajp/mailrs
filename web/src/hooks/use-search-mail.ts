import { useSetAtom } from 'jotai'
import { useCallback } from 'react'
import { useNavigate } from 'react-router'

import { searchQueryAtom } from '@/store/ui'

/// Search the mail for `query`: the mail page's lists all narrow by
/// `searchQueryAtom`, so a search started anywhere else writes it and
/// goes there. The palette used to go to `/?q=`, which nothing reads.
export function useSearchMail(): (query: string) => void {
  const setSearchQuery = useSetAtom(searchQueryAtom)
  const navigate = useNavigate()
  return useCallback(
    (query: string) => {
      setSearchQuery(query.trim())
      navigate('/mail')
    },
    [setSearchQuery, navigate]
  )
}
