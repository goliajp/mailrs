// responsive table wrapper: horizontal scroll on mobile, border + rounded corners
export function ScrollableTable({ children }: { children: React.ReactNode }) {
  return <div className="border-border overflow-x-auto rounded-lg border">{children}</div>
}
