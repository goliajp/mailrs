type AdminSectionProps = {
  children: React.ReactNode
  title: string
}

/** A titled, bordered block of an admin page — one group of settings or figures. */
export function AdminSection({ children, title }: AdminSectionProps) {
  return (
    <section aria-label={title} className="border-border overflow-hidden rounded-lg border">
      <div className="border-border bg-bg-secondary border-b px-4 py-2.5">
        <h3 className="text-sm font-semibold">{title}</h3>
      </div>
      {children}
    </section>
  )
}
