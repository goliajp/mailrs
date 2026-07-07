export function Shortcut({ keys, label }: { keys: string; label: string }) {
  return (
    <span className="flex items-center gap-1.5">
      <kbd className="border-border bg-surface text-fg-secondary text-tiny rounded border px-1.5 py-0.5 font-mono">
        {keys}
      </kbd>
      <span>{label}</span>
    </span>
  )
}
