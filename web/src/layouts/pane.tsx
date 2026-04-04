// mailrs-specific pane wrappers that add the floating-panel visual style:
// raised background, rounded corners, and gaps between panels

import type { PaneGroupProps, PaneProps } from '@goliapkg/gds'

import { cx, Pane, PaneGroup } from '@goliapkg/gds'
import { forwardRef } from 'react'

export const MPane = forwardRef<HTMLDivElement, PaneProps>(function MPane(
  { className, ...props },
  ref
) {
  return (
    <Pane
      className={cx('bg-surface flex flex-col overflow-hidden md:rounded-lg', className)}
      ref={ref}
      {...props}
    />
  )
})

export const MPaneGroup = forwardRef<HTMLDivElement, PaneGroupProps>(function MPaneGroup(
  { className, ...props },
  ref
) {
  return <PaneGroup className={cx('gap-0 md:gap-1.5', className)} ref={ref} {...props} />
})
