import { useFromAddresses } from '@/lib/from-addresses'

/**
 * Which address a message leaves by.
 *
 * Only rendered when there is more than one to choose between — with a
 * single mailbox the control is furniture, and the address it would
 * show is the one already in the header.
 */
export function FromPicker({
  onChange,
  value,
}: {
  onChange: (address: string) => void
  value: string
}) {
  const { addresses } = useFromAddresses()
  if (addresses.length < 2) return null
  return (
    <label className="text-fg-muted flex items-center gap-2 text-xs">
      From
      <select
        className="border-border bg-bg text-fg rounded-md border px-2 py-1 text-xs"
        onChange={(e) => onChange(e.target.value)}
        value={value}
      >
        {addresses.map((a) => (
          <option key={a.address} value={a.address}>
            {a.label}
          </option>
        ))}
      </select>
    </label>
  )
}
