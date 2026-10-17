import { useQuery } from '@tanstack/react-query'
import { useEffect, useState } from 'react'

import { ConfirmAction } from '@/components/admin-page'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { adminKeys } from '@/lib/query-keys'
import { adminDelete, adminObjectGet, adminPost } from '@/wire/endpoints/admin'

/** The account's sieve script, editable under its row. */
export function SievePanel({ address, onClose }: { address: string; onClose: () => void }) {
  const [script, setScript] = useState('')
  const [confirming, setConfirming] = useState(false)

  const sieveKey = [...adminKeys.accounts(), 'sieve', address]

  const { data, isPending } = useQuery({
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
    deleteScript.mutate(undefined, {
      onSettled: () => setConfirming(false),
      onSuccess: () => {
        setScript('')
        onClose()
      },
    })
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
      <div className="flex gap-3 text-xs">
        <button
          className="text-accent hover:opacity-80 disabled:opacity-50"
          disabled={saveScript.isPending}
          onClick={() => saveScript.mutate(script)}
          type="button"
        >
          {pendingLabel(saveScript.isPending, 'Saving...', 'Save script')}
        </button>
        <button
          className="text-danger hover:opacity-70 disabled:opacity-50"
          disabled={deleteScript.isPending}
          onClick={() => setConfirming(true)}
          type="button"
        >
          Delete script
        </button>
        <button className="text-fg-muted hover:text-fg-secondary" onClick={onClose} type="button">
          Close
        </button>
      </div>
      {confirming && (
        <ConfirmAction
          confirmLabel={pendingLabel(deleteScript.isPending, 'Deleting...', 'Delete script')}
          consequence={`New mail to ${address} is no longer run through this script. Mail it already filed stays where it is`}
          onCancel={() => setConfirming(false)}
          onConfirm={handleDelete}
          pending={deleteScript.isPending}
          title={`Delete the sieve script of ${address}?`}
        />
      )}
    </div>
  )
}

function pendingLabel(pending: boolean, busy: string, idle: string): string {
  if (pending) return busy
  return idle
}
