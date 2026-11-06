import type React from 'react'

import { useIsMobile } from '@/hooks/use-is-mobile'
import { MPane } from '@/layouts/pane'

/**
 * The frame around a non-mail page. One copy at either width: two copies
 * hidden by CSS mounted the page twice, so radio groups named alike became
 * one group across both and every query and effect ran twice.
 */
export function PagePane({ children }: { children: React.ReactNode }) {
  const isMobile = useIsMobile()
  if (isMobile) return <div className="h-full overflow-y-auto">{children}</div>
  return <MPane className="flex p-1">{children}</MPane>
}
