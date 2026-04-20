export function DashboardShellSkeleton() {
  return (
    <div className="h-full overflow-y-auto p-4 md:p-6">
      {/* greeting row + compose */}
      <div className="mb-6 flex items-start justify-between">
        <div className="space-y-2">
          <PulseBox className="h-6 w-56" />
          <PulseBox className="h-4 w-44" />
        </div>
        <div className="flex items-center gap-2">
          <PulseBox className="h-8 w-8" />
          <PulseBox className="h-8 w-24" />
        </div>
      </div>
      {/* search bar */}
      <PulseBox className="mb-6 h-10 w-full" />
      {/* 4 stat cards */}
      <div className="mb-6 grid grid-cols-2 gap-3 lg:grid-cols-4">
        {Array.from({ length: 4 }).map((_, i) => (
          <div
            className="border-border flex items-center gap-3 rounded-lg border px-4 py-3"
            key={i}
          >
            <PulseBox className="h-9 w-9" />
            <div className="flex-1 space-y-1.5">
              <PulseBox className="h-6 w-10" />
              <PulseBox className="h-3 w-14" />
            </div>
          </div>
        ))}
      </div>
      {/* main grid: 2/3 left + 1/3 right */}
      <div className="grid gap-6 lg:grid-cols-3">
        <div className="space-y-6 lg:col-span-2">
          {/* big inbox status box */}
          <div className="border-border overflow-hidden rounded-lg border">
            <SkeletonHeader withAction={false} />
            <div className="flex flex-col items-center gap-2 px-4 py-6">
              <PulseBox className="h-8 w-8 rounded-full" />
              <PulseBox className="mt-1 h-3 w-56" />
              <PulseBox className="mt-2 h-7 w-32" />
            </div>
          </div>
          {/* recent activity rows */}
          <div className="border-border overflow-hidden rounded-lg border">
            <SkeletonHeader />
            <div className="space-y-0.5 p-2">
              {Array.from({ length: 5 }).map((_, i) => (
                <div className="flex items-center gap-3 px-2 py-2" key={i}>
                  <PulseBox className="h-8 w-8 rounded-full" />
                  <div className="flex-1 space-y-1.5">
                    <PulseBox className="h-3.5 w-1/3" />
                    <PulseBox className="h-3 w-2/3" />
                  </div>
                  <PulseBox className="h-3 w-10" />
                </div>
              ))}
            </div>
          </div>
        </div>
        <div className="space-y-6">
          {/* categories with bars */}
          <div className="border-border overflow-hidden rounded-lg border">
            <SkeletonHeader withAction={false} />
            <div className="space-y-2.5 p-3">
              {Array.from({ length: 6 }).map((_, i) => (
                <div className="space-y-1" key={i}>
                  <div className="flex items-center justify-between">
                    <PulseBox className="h-3 w-20" />
                    <PulseBox className="h-3 w-12" />
                  </div>
                  <div className="bg-bg-secondary h-1.5 overflow-hidden rounded-full">
                    <div
                      className="bg-border h-full animate-pulse rounded-full"
                      style={{ width: `${100 - i * 12}%` }}
                    />
                  </div>
                </div>
              ))}
            </div>
          </div>
          {/* top contacts */}
          <div className="border-border overflow-hidden rounded-lg border">
            <SkeletonHeader withAction={false} />
            <div className="space-y-0.5 p-2">
              {Array.from({ length: 5 }).map((_, i) => (
                <div className="flex items-center gap-2.5 px-2 py-1.5" key={i}>
                  <PulseBox className="h-7 w-7 rounded-full" />
                  <div className="flex-1 space-y-1">
                    <PulseBox className="h-3.5 w-2/5" />
                    <PulseBox className="h-3 w-3/5" />
                  </div>
                  <PulseBox className="h-4 w-6 rounded-full" />
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}

// shared skeleton used by both phases of dashboard loading:
//   1) Suspense fallback while the lazy dashboard chunk is downloading
//      (registered in app.tsx) — entry chunk paints this at FCP
//   2) dashboard.tsx itself returns this while data is fetching
// using the same component for both means there is no visual jump
// between the two phases. real content swaps in once.
function PulseBox({ className }: { className: string }) {
  return <div className={`bg-border animate-pulse rounded-lg ${className}`} />
}

function SkeletonHeader({ withAction = true }: { withAction?: boolean }) {
  return (
    <div className="border-border flex items-center justify-between border-b px-4 py-2.5">
      <div className="flex items-center gap-2">
        <PulseBox className="h-4 w-4" />
        <PulseBox className="h-4 w-24" />
      </div>
      {withAction && <PulseBox className="h-3 w-14" />}
    </div>
  )
}
