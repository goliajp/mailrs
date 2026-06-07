import { ScrollableTable } from '@/components/scrollable-table'

type AdminTableSkeletonProps = {
  cols: number
  headers?: string[]
  rows?: number
}

export function AdminTableSkeleton({ cols, headers, rows = 5 }: AdminTableSkeletonProps) {
  const colCount = headers?.length ?? cols
  return (
    <ScrollableTable>
      <table aria-busy="true" className="w-full text-left text-sm">
        {headers && (
          <thead className="border-border bg-bg-secondary border-b">
            <tr>
              {headers.map((h) => (
                <th className="px-4 py-2.5 font-medium" key={h}>
                  {h}
                </th>
              ))}
            </tr>
          </thead>
        )}
        <tbody>
          {Array.from({ length: rows }).map((_, r) => (
            <tr className="border-border border-b last:border-0" key={r}>
              {Array.from({ length: colCount }).map((__, c) => (
                <td className="px-4 py-3" key={c}>
                  <span className="bg-bg-secondary block h-4 w-3/4 animate-pulse rounded" />
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </ScrollableTable>
  )
}
