import type { AgentKey } from '../_shared'
import type { SortDir, SortField } from './table-model'

import { ArrowDown, ArrowUp, ChevronsUpDown, Trash2 } from 'lucide-react'
import { useEffect, useRef } from 'react'

import { CopyButton } from './copy-button'
import { formatAbsolute, formatRelative, parseEpochSeconds } from './format'
import { describeScopes } from './scopes'

const CELL = 'border-border/60 border-b px-3 py-2.5 align-middle'

/**
 * The API keys data grid. Header cells are sticky against the table's own
 * scrollport so the column meaning survives a long page.
 */
export function KeyTable({
  now,
  onRevoke,
  onSort,
  onToggleAll,
  onToggleRow,
  rows,
  selected,
  sortDir,
  sortField,
}: {
  now: Date
  onRevoke: (key: AgentKey) => void
  onSort: (field: SortField) => void
  onToggleAll: (checked: boolean) => void
  onToggleRow: (id: string, checked: boolean) => void
  rows: readonly AgentKey[]
  selected: ReadonlySet<string>
  sortDir: SortDir
  sortField: SortField
}) {
  const allChecked = rows.length > 0 && rows.every((row) => selected.has(row.id))
  const someChecked = rows.some((row) => selected.has(row.id))

  return (
    <div className="border-border overflow-hidden rounded-lg border">
      <div className="max-h-[60vh] overflow-auto">
        <table className="w-full border-separate border-spacing-0 text-left">
          <thead>
            <tr className="bg-bg-secondary sticky top-0 z-10">
              <th className={`${CELL} w-10`} scope="col">
                <HeaderCheckbox
                  checked={allChecked}
                  indeterminate={someChecked && !allChecked}
                  onChange={onToggleAll}
                />
              </th>
              <SortHeader
                dir={sortDir}
                field="name"
                label="Name"
                onSort={onSort}
                sortField={sortField}
              />
              <SortHeader
                dir={sortDir}
                field="prefix"
                label="Key"
                onSort={onSort}
                sortField={sortField}
              />
              <SortHeader
                dir={sortDir}
                field="scopes"
                label="Access"
                onSort={onSort}
                sortField={sortField}
              />
              <SortHeader
                dir={sortDir}
                field="created"
                label="Created"
                onSort={onSort}
                sortField={sortField}
              />
              <SortHeader
                dir={sortDir}
                field="id"
                label="ID"
                onSort={onSort}
                sortField={sortField}
              />
              <th className={`${CELL} w-24`} scope="col">
                <span className="sr-only">Actions</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <KeyRow
                key={row.id}
                now={now}
                onRevoke={() => onRevoke(row)}
                onToggle={(checked) => onToggleRow(row.id, checked)}
                row={row}
                selected={selected.has(row.id)}
              />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}

function ariaSort(active: boolean, dir: SortDir): 'ascending' | 'descending' | 'none' {
  if (!active) return 'none'
  if (dir === 'asc') return 'ascending'
  return 'descending'
}

function CreatedCell({ now, raw }: { now: Date; raw: string }) {
  const date = parseEpochSeconds(raw)
  if (!date) return <span className="text-fg-muted">—</span>
  return (
    <span className="flex flex-col leading-tight">
      <span className="tabular-nums select-text">{formatAbsolute(date)}</span>
      <span className="text-fg-muted text-mini">{formatRelative(date, now)}</span>
    </span>
  )
}

function HeaderCheckbox({
  checked,
  indeterminate,
  onChange,
}: {
  checked: boolean
  indeterminate: boolean
  onChange: (checked: boolean) => void
}) {
  const ref = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (ref.current) ref.current.indeterminate = indeterminate
  }, [indeterminate])

  return (
    <input
      aria-label="Select all keys on this page"
      checked={checked}
      className="accent-accent align-middle"
      onChange={(e) => onChange(e.target.checked)}
      ref={ref}
      type="checkbox"
    />
  )
}

function KeyRow({
  now,
  onRevoke,
  onToggle,
  row,
  selected,
}: {
  now: Date
  onRevoke: () => void
  onToggle: (checked: boolean) => void
  row: AgentKey
  selected: boolean
}) {
  return (
    <tr className={rowClass(selected)}>
      <td className={CELL}>
        <input
          aria-label={`Select ${row.name}`}
          checked={selected}
          className="accent-accent align-middle"
          onChange={(e) => onToggle(e.target.checked)}
          type="checkbox"
        />
      </td>
      <td className={`${CELL} text-sm font-medium select-text`}>{row.name}</td>
      <td className={CELL}>
        <span className="flex items-center gap-1">
          <code className="text-fg-secondary text-mid font-mono select-text">{row.prefix}</code>
          <span className="text-fg-muted text-mid font-mono">…</span>
          <CopyButton
            className="opacity-60 hover:opacity-100"
            label="key prefix"
            value={row.prefix}
          />
        </span>
      </td>
      <td className={CELL}>
        <ScopeBadge scopes={row.scopes} />
      </td>
      <td className={`${CELL} text-mid`}>
        <CreatedCell now={now} raw={row.created_at} />
      </td>
      <td className={`${CELL} text-fg-muted text-mid font-mono tabular-nums select-text`}>
        {row.id}
      </td>
      <td className={`${CELL} text-right`}>
        <button
          className="text-danger hover:bg-danger/10 inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-xs opacity-80 transition-colors hover:opacity-100"
          onClick={onRevoke}
          type="button"
        >
          <Trash2 aria-hidden className="h-3.5 w-3.5" />
          Revoke
        </button>
      </td>
    </tr>
  )
}

function rowClass(selected: boolean): string {
  if (selected) return 'bg-accent/5'
  return 'hover:bg-bg-secondary/60 transition-colors'
}

function ScopeBadge({ scopes }: { scopes: readonly string[] }) {
  const described = describeScopes(scopes)
  return <span className={scopeClass(described.tone)}>{described.label}</span>
}

function scopeClass(tone: 'elevated' | 'normal'): string {
  const base = 'inline-flex rounded px-1.5 py-0.5 font-mono text-mini select-text'
  if (tone === 'elevated') return `${base} bg-warning/15 text-warning`
  return `${base} bg-bg-secondary text-fg-secondary`
}

function SortHeader({
  dir,
  field,
  label,
  onSort,
  sortField,
}: {
  dir: SortDir
  field: SortField
  label: string
  onSort: (field: SortField) => void
  sortField: SortField
}) {
  const active = field === sortField
  return (
    <th aria-sort={ariaSort(active, dir)} className={CELL} scope="col">
      <button
        className="text-fg-muted hover:text-fg text-mini inline-flex items-center gap-1 font-semibold tracking-wider uppercase transition-colors"
        onClick={() => onSort(field)}
        type="button"
      >
        {label}
        <SortIcon active={active} dir={dir} />
      </button>
    </th>
  )
}

function SortIcon({ active, dir }: { active: boolean; dir: SortDir }) {
  if (!active) return <ChevronsUpDown aria-hidden className="h-3 w-3 opacity-40" />
  if (dir === 'asc') return <ArrowUp aria-hidden className="text-accent h-3 w-3" />
  return <ArrowDown aria-hidden className="text-accent h-3 w-3" />
}
