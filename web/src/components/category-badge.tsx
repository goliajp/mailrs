const styles: Record<string, { bg: string; text: string; label: string }> = {
  personal: { bg: 'bg-[var(--color-status-success-subtle)]', text: 'text-[var(--color-status-success)]', label: 'Personal' },
  general: { bg: 'bg-[var(--color-bg-raised)]', text: 'text-[var(--color-text-secondary)]', label: 'General' },
  notification: { bg: 'bg-[var(--color-brand-subtle)]', text: 'text-[var(--color-brand-primary)]', label: 'Notification' },
  promotion: { bg: 'bg-orange-100 dark:bg-orange-900/30', text: 'text-orange-700 dark:text-orange-400', label: 'Promotion' },
  newsletter: { bg: 'bg-[var(--color-status-danger-subtle)]', text: 'text-[var(--color-status-danger)]', label: 'Newsletter' },
  receipt: { bg: 'bg-emerald-100 dark:bg-emerald-900/30', text: 'text-emerald-700 dark:text-emerald-400', label: 'Receipt' },
  shipping: { bg: 'bg-teal-100 dark:bg-teal-900/30', text: 'text-teal-700 dark:text-teal-400', label: 'Shipping' },
  travel: { bg: 'bg-cyan-100 dark:bg-cyan-900/30', text: 'text-cyan-700 dark:text-cyan-400', label: 'Travel' },
  finance: { bg: 'bg-yellow-100 dark:bg-yellow-900/30', text: 'text-yellow-700 dark:text-yellow-400', label: 'Finance' },
  work: { bg: 'bg-[var(--color-brand-subtle)]', text: 'text-[var(--color-brand-primary)]', label: 'Work' },
  spam: { bg: 'bg-amber-100 dark:bg-amber-900/30', text: 'text-amber-700 dark:text-amber-400', label: 'Spam' },
  scam: { bg: 'bg-[var(--color-status-danger-subtle)]', text: 'text-[var(--color-status-danger)]', label: 'Scam' },
}

export function CategoryBadge({ category }: { category: string }) {
  const s = styles[category] ?? { bg: 'bg-[var(--color-bg-raised)]', text: 'text-[var(--color-text-tertiary)]', label: category }
  return (
    <span className={`inline-flex select-none items-center rounded px-1.5 py-0.5 text-[11px] font-medium capitalize ${s.bg} ${s.text}`}>
      {s.label}
    </span>
  )
}

// eslint-disable-next-line react-refresh/only-export-components
export function riskColor(score: number): string {
  if (score >= 60) return 'text-red-500'
  if (score >= 40) return 'text-amber-500'
  if (score >= 15) return 'text-blue-500'
  return 'text-green-500'
}

const importanceStyles: Record<string, { bg: string; text: string; label: string; icon: string }> = {
  critical: { bg: 'bg-[var(--color-status-danger-subtle)]', text: 'text-[var(--color-status-danger)]', label: 'Critical', icon: '!!' },
  important: { bg: 'bg-amber-100 dark:bg-amber-900/30', text: 'text-amber-700 dark:text-amber-400', label: 'Important', icon: '!' },
  normal: { bg: 'bg-[var(--color-bg-raised)]', text: 'text-[var(--color-text-tertiary)]', label: 'Normal', icon: '' },
  low: { bg: 'bg-[var(--color-bg-sunken)]', text: 'text-[var(--color-text-tertiary)]', label: 'Low', icon: '' },
  noise: { bg: 'bg-[var(--color-bg-sunken)]', text: 'text-zinc-300 dark:text-zinc-600', label: 'Noise', icon: '' },
}

export function ImportanceBadge({ level }: { level: string }) {
  if (!level || level === 'normal') return null
  const s = importanceStyles[level] ?? importanceStyles.normal
  return (
    <span className={`inline-flex select-none items-center gap-0.5 rounded px-1.5 py-0.5 text-[11px] font-medium ${s.bg} ${s.text}`}>
      {s.icon && <span className="font-bold">{s.icon}</span>}
      {s.label}
    </span>
  )
}

export function ActionBadge() {
  return (
    <span className="inline-flex select-none items-center gap-0.5 rounded bg-purple-100 px-1.5 py-0.5 text-[11px] font-medium text-purple-700 dark:bg-purple-900/30 dark:text-purple-400">
      Action
    </span>
  )
}

export function IntentBadge({ intent }: { intent: string }) {
  if (!intent || intent === 'inform') return null
  const intentStyles: Record<string, { bg: string; text: string }> = {
    request: { bg: 'bg-purple-100 dark:bg-purple-900/30', text: 'text-purple-700 dark:text-purple-400' },
    confirm: { bg: 'bg-emerald-100 dark:bg-emerald-900/30', text: 'text-emerald-700 dark:text-emerald-400' },
    social: { bg: 'bg-pink-100 dark:bg-pink-900/30', text: 'text-pink-700 dark:text-pink-400' },
    alert: { bg: 'bg-[var(--color-status-danger-subtle)]', text: 'text-[var(--color-status-danger)]' },
  }
  const s = intentStyles[intent] ?? { bg: 'bg-[var(--color-bg-raised)]', text: 'text-[var(--color-text-tertiary)]' }
  return (
    <span className={`inline-flex select-none items-center rounded px-1.5 py-0.5 text-[11px] font-medium capitalize ${s.bg} ${s.text}`}>
      {intent}
    </span>
  )
}
