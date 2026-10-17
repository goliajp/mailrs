/** The way back up one level of the mail audit; the level lives in the URL. */
export function BackButton({ label, onBack }: { label: string; onBack: () => void }) {
  return (
    <button
      className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
      onClick={onBack}
      type="button"
    >
      {label}
    </button>
  )
}
