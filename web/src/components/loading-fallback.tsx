export function LoadingFallback() {
  return (
    <div className="text-fg-muted flex flex-1 flex-col items-center justify-center gap-3 p-8">
      <div className="border-border border-t-accent h-8 w-8 animate-spin rounded-full border-2" />
      <span className="text-sm">Loading…</span>
    </div>
  )
}
