import { useState } from 'react'

import { postJson } from '@/lib/api'

type Preview = {
  height: number
  image_url: string
  name: string
  width: number
}

type RenderResult = {
  error?: string
  errors?: string[]
  previews?: Preview[]
}

const PRESET_LABELS: Record<string, string> = {
  desktop: 'Desktop (660px)',
  gmail: 'Gmail',
  mobile: 'Mobile (375px)',
  outlook: 'Outlook',
}

export function RenderPreview({ html }: { html: string }) {
  const [previews, setPreviews] = useState<Preview[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const [activeTab, setActiveTab] = useState('desktop')

  const render = async () => {
    setLoading(true)
    setError('')
    try {
      const result = await postJson<RenderResult>('/mail/render-preview', {
        html,
        presets: ['desktop', 'mobile', 'gmail', 'outlook'],
      })

      if (result.error) {
        setError(result.error)
      } else if (result.previews && result.previews.length > 0) {
        setPreviews(result.previews)
        setActiveTab(result.previews[0].name)
        if (result.errors && result.errors.length > 0) {
          setError(
            `${result.errors.length} preset(s) failed: ${result.errors.join('; ')}`
          )
        }
      } else {
        const detail = result.errors?.join('; ') ?? 'unknown'
        setError(`No previews generated: ${detail}`)
      }
    } catch {
      setError('Render failed')
    } finally {
      setLoading(false)
    }
  }

  if (previews.length === 0 && !loading && !error) {
    return (
      <button
        className="rounded-md bg-[var(--color-bg-raised)] px-3 py-1.5 text-xs text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)]"
        onClick={render}
      >
        Preview in clients
      </button>
    )
  }

  const active = previews.find((p) => p.name === activeTab)

  return (
    <div className="flex flex-col gap-2">
      {loading && (
        <div className="flex items-center gap-2 text-xs text-[var(--color-text-tertiary)]">
          <div className="h-3 w-3 animate-spin rounded-full border border-[var(--color-border-default)] border-t-[var(--color-brand-primary)]" />
          Rendering previews...
        </div>
      )}
      {error && (
        <p className="text-xs text-[var(--color-status-danger)]">{error}</p>
      )}
      {previews.length > 0 && (
        <>
          <div className="flex gap-1">
            {previews.map((p) => (
              <button
                className={`rounded-md px-2 py-1 text-xs transition-colors ${
                  activeTab === p.name
                    ? 'bg-[var(--color-brand-primary)] text-white'
                    : 'bg-[var(--color-bg-raised)] text-[var(--color-text-secondary)] hover:bg-[var(--color-hover)]'
                }`}
                key={p.name}
                onClick={() => setActiveTab(p.name)}
              >
                {PRESET_LABELS[p.name] ?? p.name}
              </button>
            ))}
          </div>
          {active && (
            <div className="overflow-auto rounded-md border border-[var(--color-border-default)] bg-white">
              <img
                alt={`${active.name} preview`}
                className="block"
                src={active.image_url}
              />
            </div>
          )}
        </>
      )}
    </div>
  )
}
