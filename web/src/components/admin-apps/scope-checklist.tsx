export function ScopeChecklist({
  onChange,
  permissions,
  selected,
}: {
  onChange: (next: string[]) => void
  permissions: string[]
  selected: string[]
}) {
  const toggle = (perm: string) => {
    if (selected.includes(perm)) {
      onChange(selected.filter((s) => s !== perm))
      return
    }
    onChange([...selected, perm])
  }
  return (
    <div className="flex flex-wrap gap-x-4 gap-y-2">
      {permissions.map((perm) => (
        <label className="flex items-center gap-1.5 text-sm" key={perm}>
          <input checked={selected.includes(perm)} onChange={() => toggle(perm)} type="checkbox" />
          {perm}
        </label>
      ))}
    </div>
  )
}
