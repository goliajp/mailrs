type BrandHeaderProps = {
  subtitle?: string
}

export function BrandHeader({ subtitle }: BrandHeaderProps) {
  return (
    <div className="flex flex-col items-center">
      <img alt="mailrs" className="mb-3 h-14 w-14 rounded-lg shadow-sm" src="/icon.svg" />
      <h1 className="text-fg text-xl font-semibold tracking-tight">mailrs</h1>
      {subtitle && <p className="text-fg-muted mt-1 text-sm">{subtitle}</p>}
    </div>
  )
}
