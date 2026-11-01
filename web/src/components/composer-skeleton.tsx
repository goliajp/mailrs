/** Placeholder shown while the lazy rich-text composer loads. */
export function ComposerSkeleton() {
  return (
    <div aria-busy="true" className="flex h-full flex-col gap-2 p-4">
      <div className="bg-bg-secondary h-4 w-3/4 animate-pulse rounded" />
      <div className="bg-bg-secondary h-4 w-1/2 animate-pulse rounded" />
      <div className="bg-bg-secondary mt-2 h-24 w-full animate-pulse rounded" />
    </div>
  )
}
