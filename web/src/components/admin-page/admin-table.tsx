import { Fragment } from 'react'

import { ScrollableTable } from '@/components/scrollable-table'
import { cn } from '@/lib/cn'

export type Column<T> = {
  /** right-aligned with tabular figures: counts, sizes, times */
  align?: 'end'
  cell: (row: T) => React.ReactNode
  /** visually hidden header text, for an actions column */
  header: string
  hideHeader?: boolean
  key: string
}

/**
 * The one table every admin list uses: quiet headers, numbers on the
 * right, row actions in the last column.
 */
export function AdminTable<T>({
  columns,
  detail,
  label,
  rowKey,
  rows,
}: {
  columns: Column<T>[]
  /** a full-width row under a row, e.g. its check report; null for none */
  detail?: (row: T) => React.ReactNode
  /** what the table lists, for screen readers */
  label: string
  rowKey: (row: T) => string
  rows: readonly T[]
}) {
  return (
    <ScrollableTable>
      <table aria-label={label} className="w-full text-left text-sm">
        <thead className="border-border border-b">
          <tr>
            {columns.map((c) => (
              <th
                className={cn('text-fg-muted px-4 py-2 text-xs font-normal', alignClass(c.align))}
                key={c.key}
                scope="col"
              >
                <HeaderText column={c} />
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <Fragment key={rowKey(row)}>
              <tr className="border-border hover:bg-bg-secondary/60 border-b last:border-0">
                {columns.map((c) => (
                  <td className={cn('px-4 py-2.5', alignClass(c.align))} key={c.key}>
                    {c.cell(row)}
                  </td>
                ))}
              </tr>
              <DetailRow content={detail?.(row)} span={columns.length} />
            </Fragment>
          ))}
        </tbody>
      </table>
    </ScrollableTable>
  )
}

function alignClass(align: Column<unknown>['align']): string {
  if (align === 'end') return 'text-right tabular-nums'
  return ''
}

function DetailRow({ content, span }: { content: React.ReactNode; span: number }) {
  if (content === null || content === undefined) return null
  return (
    <tr className="border-border border-b last:border-0">
      <td className="px-4 pt-1 pb-4" colSpan={span}>
        {content}
      </td>
    </tr>
  )
}

function HeaderText<T>({ column }: { column: Column<T> }) {
  if (column.hideHeader) return <span className="sr-only">{column.header}</span>
  return <>{column.header}</>
}
