import { toast } from '@goliapkg/gds'

// Backend: crates/webapi/src/handlers/apps_keys/apps.rs — fn create_app
export type CreatedApp = {
  app_id: string
  id: number
  secret: string
}

/** The one response that carries the secret; the backend keeps only its SHA-256. */
export function CreatedSecret({ app, onDismiss }: { app: CreatedApp; onDismiss: () => void }) {
  const copy = () => {
    navigator.clipboard.writeText(app.secret).then(() => toast.success('Secret copied'))
  }
  return (
    <div className="border-warning bg-warning/10 mb-4 rounded-lg border p-4" role="status">
      <p className="mb-2 text-sm font-semibold">Secret for {app.app_id}</p>
      <p className="text-fg-secondary mb-2 text-xs">
        Shown once. Only its SHA-256 hash is stored, so copy it now. No request signs in with an app
        secret yet; keep it for the integration that will
      </p>
      <div className="flex items-center gap-2">
        <code className="bg-bg-secondary flex-1 rounded px-3 py-1.5 font-mono text-sm break-all">
          {app.secret}
        </code>
        <button
          className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm"
          onClick={copy}
          type="button"
        >
          Copy
        </button>
      </div>
      <button
        className="text-fg-secondary mt-2 text-xs transition-colors hover:opacity-70"
        onClick={onDismiss}
        type="button"
      >
        Dismiss
      </button>
    </div>
  )
}
