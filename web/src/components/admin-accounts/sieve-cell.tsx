import { useQuery } from '@tanstack/react-query'
import { useEffect, useState } from 'react'

import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { adminKeys } from '@/lib/query-keys'
import { adminDelete, adminObjectGet, adminPost } from '@/wire/endpoints/admin'

export function SieveCell({ address }: { address: string }) {
  const [open, setOpen] = useState(false)
  const [script, setScript] = useState('')

  const sieveKey = [...adminKeys.accounts(), 'sieve', address]

  const { data, isPending } = useQuery({
    enabled: open,
    queryKey: sieveKey,
    queryFn: async ({ signal }) => {
      try {
        const result = await adminObjectGet<{ script: string }>(
          `/admin/accounts/${encodeURIComponent(address)}/sieve`,
          signal
        )
        return result.script ?? ''
      } catch {
        return ''
      }
    },
  })

  useEffect(() => {
    if (data !== undefined) setScript(data)
  }, [data])

  const saveScript = useAdminMutation({
    invalidateKey: sieveKey,
    successMsg: 'Sieve script saved',
    mutationFn: (text: string) =>
      adminPost(`/admin/accounts/${encodeURIComponent(address)}/sieve`, { script: text }),
  })

  const deleteScript = useAdminMutation({
    invalidateKey: sieveKey,
    successMsg: 'Sieve script deleted',
    mutationFn: () => adminDelete(`/admin/accounts/${encodeURIComponent(address)}/sieve`),
  })

  const handleDelete = () => {
    if (!window.confirm('Delete this sieve script? This cannot be undone.')) return
    deleteScript.mutate(undefined, {
      onSuccess: () => {
        setScript('')
        setOpen(false)
      },
    })
  }

  if (!open) {
    return (
      <button className="text-accent text-xs hover:opacity-80" onClick={() => setOpen(true)}>
        Sieve
      </button>
    )
  }

  if (isPending) {
    return <span className="text-fg-muted text-xs">Loading...</span>
  }

  const busy = saveScript.isPending || deleteScript.isPending

  return (
    <div className="space-y-2">
      <textarea
        aria-label="Sieve script"
        className="border-border bg-bg-secondary w-full rounded-md border px-2 py-1.5 font-mono text-xs"
        disabled={busy}
        onChange={(e) => setScript(e.target.value)}
        placeholder='require "fileinto"; ...'
        rows={6}
        value={script}
      />
      {saveScript.error instanceof Error && (
        <p className="text-danger text-xs">{saveScript.error.message}</p>
      )}
      <div className="flex gap-1.5">
        <button
          className="text-success text-xs hover:opacity-80 disabled:opacity-50"
          disabled={saveScript.isPending}
          onClick={() => saveScript.mutate(script)}
        >
          {saveScript.isPending ? '...' : 'Save'}
        </button>
        <button
          className="text-danger text-xs hover:opacity-80 disabled:opacity-50"
          disabled={deleteScript.isPending}
          onClick={handleDelete}
        >
          {deleteScript.isPending ? '...' : 'Delete'}
        </button>
        <button
          className="text-fg-muted hover:text-fg-secondary text-xs"
          onClick={() => setOpen(false)}
        >
          Close
        </button>
      </div>
    </div>
  )
}
