import type { MockNamespace } from '@/wire/schemas/mock'

import { toast } from '@goliapkg/gds'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Trash2 } from 'lucide-react'
import { useState } from 'react'

import { bytes } from '@/components/admin-overview/flow-model'
import { cn } from '@/lib/cn'
import { mockKeys } from '@/lib/query-keys'
import { wireCreateMockNamespace, wireDeleteMockNamespace } from '@/wire/endpoints/mock'

import { btnPrimary, ConfirmDialog, inputClass } from '../settings/_shared'
import { errorText } from './errors'

/** The caller's namespaces, a create form above them, delete behind a confirm. */
export function NamespaceList({
  everyone,
  namespaces,
  onSelect,
  selected,
}: {
  /** listing every user's namespaces: show owners, hide the create form */
  everyone: boolean
  namespaces: readonly MockNamespace[]
  onSelect: (name: null | string) => void
  selected: null | string
}) {
  const queryClient = useQueryClient()
  const [name, setName] = useState('')
  const [deleting, setDeleting] = useState<null | string>(null)
  const invalidate = () => queryClient.invalidateQueries({ queryKey: mockKeys.namespaces() })

  const create = useMutation({
    mutationFn: (n: string) => wireCreateMockNamespace(n),
    onError: (e) => toast.error(errorText(e, 'Could not create the namespace')),
    onSuccess: (created) => {
      setName('')
      onSelect(created.name)
      void invalidate()
    },
  })

  const remove = useMutation({
    mutationFn: (n: string) => wireDeleteMockNamespace(n),
    onError: (e) => toast.error(errorText(e, 'Could not delete the namespace')),
    onSettled: () => {
      setDeleting(null)
      void invalidate()
    },
    onSuccess: (_, n) => {
      if (n === selected) onSelect(null)
      toast.success(`Deleted ${n}`)
    },
  })

  const submit = (e: React.FormEvent) => {
    e.preventDefault()
    const trimmed = name.trim()
    if (trimmed === '' || create.isPending) return
    create.mutate(trimmed)
  }

  return (
    <section aria-label="Namespaces" className="space-y-3">
      {!everyone && (
        <>
          <form className="flex gap-2" onSubmit={submit}>
            <input
              aria-label="New namespace name"
              className={inputClass}
              onChange={(e) => setName(e.target.value)}
              placeholder="my-app"
              value={name}
            />
            <button
              className={cn(btnPrimary, 'shrink-0')}
              disabled={name.trim() === '' || create.isPending}
              type="submit"
            >
              Create
            </button>
          </form>
          <p className="text-fg-muted text-mini">
            3–32 characters of a-z, 0-9 and -, unique across the server
          </p>
        </>
      )}

      <ul className="space-y-1.5">
        {namespaces.map((ns) => (
          <NamespaceRow
            active={ns.name === selected}
            key={ns.name}
            namespace={ns}
            onDelete={() => setDeleting(ns.name)}
            onSelect={() => onSelect(ns.name)}
            showOwner={everyone}
          />
        ))}
      </ul>

      {deleting !== null && (
        <ConfirmDialog
          message={`Delete namespace ${deleting} and every message in it? Its addresses stop receiving mail.`}
          onCancel={() => setDeleting(null)}
          onConfirm={() => remove.mutate(deleting)}
        />
      )}
    </section>
  )
}

function NamespaceRow({
  active,
  namespace,
  onDelete,
  onSelect,
  showOwner,
}: {
  active: boolean
  namespace: MockNamespace
  onDelete: () => void
  onSelect: () => void
  showOwner: boolean
}) {
  return (
    <li
      className={cn(
        'group flex items-start gap-2 rounded-md border px-3 py-2 transition-colors',
        active && 'border-accent bg-accent/5',
        !active && 'border-border hover:bg-bg-secondary'
      )}
    >
      <button
        aria-current={active || undefined}
        className="min-w-0 flex-1 text-left"
        onClick={onSelect}
        type="button"
      >
        <span className="block truncate text-sm font-medium">{namespace.name}</span>
        <span className="text-fg-muted block truncate font-mono text-xs">{namespace.address}</span>
        {showOwner && (
          <span className="text-fg-secondary block truncate text-xs">{namespace.owner}</span>
        )}
        <span className="text-fg-muted text-mini block">
          {namespace.messages} messages, {bytes(namespace.bytes)}, {namespace.dropped} dropped
        </span>
      </button>
      <button
        aria-label={`Delete namespace ${namespace.name}`}
        className="text-fg-muted hover:text-danger hover:bg-bg-secondary rounded-md p-1 transition-colors"
        onClick={onDelete}
        type="button"
      >
        <Trash2 aria-hidden className="h-4 w-4" />
      </button>
    </li>
  )
}
