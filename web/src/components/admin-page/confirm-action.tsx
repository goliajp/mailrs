import { MobileModal } from '@/components/mobile-modal'

/**
 * A confirmation that says what will happen. The confirm button carries
 * the action's own verb ("Delete domain"), never a bare "Confirm".
 */
export function ConfirmAction({
  confirmLabel,
  consequence,
  onCancel,
  onConfirm,
  pending,
  title,
}: {
  confirmLabel: string
  /** what is lost or changed, in one or two sentences */
  consequence: string
  onCancel: () => void
  onConfirm: () => void
  pending?: boolean
  title: string
}) {
  return (
    <MobileModal onClose={onCancel} open>
      <div
        aria-label={title}
        className="bg-surface w-full max-w-sm rounded-lg p-6 shadow-lg"
        role="alertdialog"
      >
        <h3 className="text-fg text-sm font-medium">{title}</h3>
        <p className="text-fg-secondary mt-2 text-sm">{consequence}</p>
        <div className="mt-5 flex justify-end gap-2">
          <button
            className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
            onClick={onCancel}
            type="button"
          >
            Cancel
          </button>
          <button
            className="bg-danger rounded-md px-3 py-1.5 text-sm font-medium text-white transition-colors hover:opacity-90 disabled:opacity-50"
            disabled={pending}
            onClick={onConfirm}
            type="button"
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </MobileModal>
  )
}
