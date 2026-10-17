import type { MockMintedAddress } from '@/wire/schemas/mock'

import { toast } from '@goliapkg/gds'
import { useMutation } from '@tanstack/react-query'
import { AtSign } from 'lucide-react'
import { useState } from 'react'

import { CopyButton } from '@/components/settings/api-keys/copy-button'
import { copyText } from '@/components/settings/api-keys/copy-text'
import { cn } from '@/lib/cn'
import { wireMintMockAddress } from '@/wire/endpoints/mock'

import { btnPrimary, inputClass } from '../settings/_shared'
import { errorText } from './errors'

/**
 * Mints `{ns}.{tag}@domain` and puts it on the clipboard. Any tag already
 * works without minting; minting records when the address was handed out,
 * which is what `wait` measures "new mail" from.
 */
export function AddressMinter({ namespace }: { namespace: string }) {
  const [tag, setTag] = useState('')
  // the last POST response, shown until the next one
  const [minted, setMinted] = useState<MockMintedAddress | null>(null)

  const mint = useMutation({
    mutationFn: (t: string) => wireMintMockAddress(namespace, optionalTag(t)),
    onError: (e) => toast.error(errorText(e, 'Could not mint an address')),
    onSuccess: (out) => {
      setMinted(out)
      setTag('')
      void copyText(out.address, 'Address')
    },
  })

  const submit = (e: React.FormEvent) => {
    e.preventDefault()
    if (mint.isPending) return
    mint.mutate(tag)
  }

  const shown = mintedFor(minted, namespace)

  return (
    <section aria-label="New address" className="space-y-2">
      <form className="flex flex-wrap gap-2" onSubmit={submit}>
        <input
          aria-label="Tag (optional)"
          className={cn(inputClass, 'w-auto min-w-0 flex-1')}
          onChange={(e) => setTag(e.target.value)}
          placeholder="tag (optional), e.g. signup-01"
          value={tag}
        />
        <button
          className={cn(btnPrimary, 'inline-flex shrink-0 items-center gap-1.5')}
          disabled={mint.isPending}
          type="submit"
        >
          <AtSign aria-hidden className="h-4 w-4" />
          New address
        </button>
      </form>
      {shown && (
        <div className="border-border bg-bg-secondary flex items-center gap-2 rounded-md border px-3 py-1.5">
          <code className="min-w-0 flex-1 truncate text-sm" data-testid="minted-address">
            {shown.address}
          </code>
          <CopyButton label="Address" value={shown.address} withText />
        </div>
      )}
    </section>
  )
}

/** A minted address belongs to the namespace it was minted in; switching hides it. */
function mintedFor(minted: MockMintedAddress | null, namespace: string): MockMintedAddress | null {
  if (minted === null || minted.namespace !== namespace) return null
  return minted
}

function optionalTag(raw: string): string | undefined {
  const t = raw.trim()
  if (t === '') return undefined
  return t
}
