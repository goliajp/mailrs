import { toast } from '@goliapkg/gds'
import { useAtom } from 'jotai'
import { Check, Trash2 } from 'lucide-react'
import { useState } from 'react'

import { LabelDot } from '@/components/label-chip'
import { MobileModal } from '@/components/mobile-modal'
import { useConversationRows } from '@/hooks/use-current-list'
import {
  useCreateLabel,
  useDeleteLabel,
  useLabels,
  useSetThreadLabel,
  useUpdateLabel,
} from '@/hooks/use-labels'
import { labelColor } from '@/lib/label-colors'
import { labelPickerAtom } from '@/store/ui'
import { LABEL_COLORS, type WireLabel } from '@/wire/schemas/labels'

/**
 * Put labels on the conversations it was opened for, make a new one, or
 * rename, recolour and delete them. A label is ticked when every one of the
 * conversations has it.
 */
export function LabelPicker() {
  const [open, setOpen] = useAtom(labelPickerAtom)
  const [managing, setManaging] = useState(false)
  const close = () => {
    setOpen(null)
    setManaging(false)
  }
  if (!open) return null
  return (
    <MobileModal className="max-w-sm" onClose={close} open>
      <div className="p-4">
        <div className="mb-3 flex items-center justify-between">
          <h3 className="text-fg text-sm font-semibold">{title(managing)}</h3>
          <button
            className="text-accent text-xs hover:underline"
            onClick={() => setManaging(!managing)}
            type="button"
          >
            {switchLabel(managing)}
          </button>
        </div>
        {managing && <ManageLabels />}
        {!managing && <ApplyLabels threadIds={open.threadIds} />}
      </div>
    </MobileModal>
  )
}

function ApplyLabels({ threadIds }: { threadIds: string[] }) {
  const { data: labels = [] } = useLabels()
  const { rows } = useConversationRows()
  const setLabel = useSetThreadLabel()
  const create = useCreateLabel()
  const [name, setName] = useState('')
  const picked = rows.filter((r) => threadIds.includes(r.thread_id))
  const onAll = (id: string) =>
    picked.length > 0 && picked.every((r) => (r.labels ?? []).includes(id))
  const toggle = (l: WireLabel) =>
    setLabel.mutate(
      { id: l.id, on: !onAll(l.id), threadIds },
      { onError: (e) => toast.error(errorText(e)) }
    )
  const add = () =>
    create.mutate(
      { color: 'gray', name },
      {
        onError: (e) => toast.error(errorText(e)),
        onSuccess: (l) => {
          setName('')
          setLabel.mutate({ id: l.id, on: true, threadIds })
        },
      }
    )
  return (
    <div>
      <ul className="max-h-72 overflow-y-auto">
        {labels.map((l) => (
          <li key={l.id}>
            <button
              aria-checked={onAll(l.id)}
              className="hover:bg-bg-secondary flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm"
              onClick={() => toggle(l)}
              role="menuitemcheckbox"
              type="button"
            >
              <span className="flex h-4 w-4 items-center justify-center">
                {onAll(l.id) && <Check className="h-3.5 w-3.5" />}
              </span>
              <LabelDot color={l.color} />
              <span className="truncate">{l.name}</span>
            </button>
          </li>
        ))}
        {labels.length === 0 && (
          <li className="text-fg-muted px-2 py-1.5 text-sm">No labels yet</li>
        )}
      </ul>
      <form
        className="border-border mt-3 flex gap-2 border-t pt-3"
        onSubmit={(e) => {
          e.preventDefault()
          if (name.trim()) add()
        }}
      >
        <input
          aria-label="New label"
          className="border-border bg-bg flex-1 rounded-md border px-2 py-1 text-sm"
          onChange={(e) => setName(e.target.value)}
          placeholder="New label"
          value={name}
        />
        <button
          className="bg-accent rounded-md px-3 py-1 text-sm font-medium text-white disabled:opacity-50"
          disabled={!name.trim() || create.isPending}
          type="submit"
        >
          Create
        </button>
      </form>
    </div>
  )
}

function errorText(err: unknown): string {
  if (err instanceof Error) return err.message
  return 'Something went wrong'
}

function ManageLabels() {
  const { data: labels = [] } = useLabels()
  return (
    <ul className="max-h-80 space-y-2 overflow-y-auto">
      {labels.map((l) => (
        <ManageRow key={l.id} label={l} />
      ))}
      {labels.length === 0 && <li className="text-fg-muted text-sm">No labels yet</li>}
    </ul>
  )
}

function ManageRow({ label }: { label: WireLabel }) {
  const update = useUpdateLabel()
  const remove = useDeleteLabel()
  const [name, setName] = useState(label.name)
  const [asking, setAsking] = useState(false)
  const save = (change: { color?: string; name?: string }) =>
    update.mutate({ change, id: label.id }, { onError: (e) => toast.error(errorText(e)) })
  return (
    <li className="border-border rounded-md border p-2">
      <div className="flex items-center gap-2">
        <input
          aria-label={`Name of ${label.name}`}
          className="border-border bg-bg flex-1 rounded-md border px-2 py-1 text-sm"
          onBlur={() => {
            if (name.trim() && name !== label.name) save({ name })
          }}
          onChange={(e) => setName(e.target.value)}
          value={name}
        />
        <button
          aria-label={`Delete ${label.name}`}
          className="text-fg-muted hover:text-danger rounded-md p-1"
          onClick={() => setAsking(true)}
          type="button"
        >
          <Trash2 className="h-4 w-4" />
        </button>
      </div>
      <div className="mt-2 flex gap-1.5">
        {LABEL_COLORS.map((c) => (
          <button
            aria-label={c}
            aria-pressed={label.color === c}
            className={swatchClass(label.color === c)}
            key={c}
            onClick={() => save({ color: c })}
            style={{ backgroundColor: labelColor(c) }}
            type="button"
          />
        ))}
      </div>
      {asking && (
        <div className="mt-2 text-xs">
          <p className="text-fg-secondary">
            Delete {label.name}? It comes off every conversation; no mail is deleted.
          </p>
          <div className="mt-1.5 flex justify-end gap-2">
            <button
              className="text-fg-secondary px-2 py-1"
              onClick={() => setAsking(false)}
              type="button"
            >
              Cancel
            </button>
            <button
              className="bg-danger rounded-md px-2 py-1 font-medium text-white"
              onClick={() => remove.mutate(label.id, { onError: (e) => toast.error(errorText(e)) })}
              type="button"
            >
              Delete
            </button>
          </div>
        </div>
      )}
    </li>
  )
}

function swatchClass(active: boolean): string {
  const base = 'h-5 w-5 rounded-full'
  if (active) return `${base} ring-fg ring-2 ring-offset-1`
  return base
}

function switchLabel(managing: boolean): string {
  if (managing) return 'Done'
  return 'Manage'
}

function title(managing: boolean): string {
  if (managing) return 'Manage labels'
  return 'Labels'
}
