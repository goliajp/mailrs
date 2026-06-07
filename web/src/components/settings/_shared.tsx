import { MobileModal } from '@/components/mobile-modal'

// shared types reused by multiple sections — colocated here so the section
// files don't import each other for type-only references.

export type AgentKey = {
  created_at: string
  expires_at: null | string
  id: string
  name: string
  prefix: string
}

export type CalendarFeed = {
  enabled: boolean
  id: number
  last_error: null | string
  last_synced_at: null | string
  name: string
  refresh_interval_secs: number
  url: string
}

export type CreatedAgentKey = {
  id: string
  key: string
  prefix: string
}

export type CreatedWebhook = {
  id: string
  signing_secret: string
}

export type KeyStatus = {
  pgp_fingerprint: null | string
  smime_fingerprint: null | string
}

export type Signature = {
  html_content: string
  id: number
  is_default: boolean
  name: string
  text_content: string
}

export type TotpSetup = {
  qr_url: string
  recovery_codes: string[]
  secret: string
}

export type TotpStatus = {
  enabled: boolean
}

export type Webhook = {
  active: boolean
  event_type: string
  filter_sender: null | string
  filter_thread_id: null | string
  id: string
  url: string
}

// shared button + input classNames — kept as constants so a future tweak
// (focus ring, density, etc.) only changes here.
export const inputClass =
  'w-full rounded-md border border-border bg-bg-secondary px-3 py-1.5 text-sm focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent/40'
export const btnPrimary =
  'rounded-md bg-fg px-3 py-1.5 text-sm font-medium text-bg transition-colors hover:opacity-90 disabled:opacity-50'
export const btnDanger =
  'rounded-md bg-danger px-3 py-1.5 text-sm font-medium text-white transition-colors hover:opacity-90 disabled:opacity-50'
export const btnSecondary =
  'rounded-md px-3 py-1.5 text-sm text-fg-secondary transition-colors hover:bg-bg-secondary'
export const cardClass = 'rounded-lg border border-border p-4'

export function ConfirmDialog({
  message,
  onCancel,
  onConfirm,
}: {
  message: string
  onCancel: () => void
  onConfirm: () => void
}) {
  return (
    <MobileModal onClose={onCancel} open>
      <div className="bg-surface w-full max-w-sm rounded-lg p-6 shadow-lg">
        <p className="text-fg-secondary mb-4 text-sm">{message}</p>
        <div className="flex justify-end gap-2">
          <button className={btnSecondary} onClick={onCancel} type="button">
            Cancel
          </button>
          <button className={btnDanger} onClick={onConfirm} type="button">
            Confirm
          </button>
        </div>
      </div>
    </MobileModal>
  )
}

export function Field({ children, label }: { children: React.ReactNode; label: string }) {
  return (
    <div className="flex items-center justify-between gap-4">
      <span className="text-fg-secondary text-sm font-medium">{label}</span>
      {children}
    </div>
  )
}

export function SectionHeader({ title }: { title: string }) {
  return <h2 className="mb-4 text-base font-semibold">{title}</h2>
}

export function Toggle({
  checked,
  onChange,
}: {
  checked: boolean
  onChange: (v: boolean) => void
}) {
  return (
    <button
      aria-checked={checked}
      className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer items-center rounded-full transition-colors ${
        checked ? 'bg-accent' : 'bg-border-strong'
      }`}
      onClick={() => onChange(!checked)}
      role="switch"
      type="button"
    >
      <span
        className={`inline-block h-4 w-4 rounded-full bg-white shadow transition-transform ${
          checked ? 'translate-x-6' : 'translate-x-1'
        }`}
      />
    </button>
  )
}
