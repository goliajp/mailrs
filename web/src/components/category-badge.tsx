const styles: Record<string, { bg: string; text: string; label: string }> = {
  personal: { bg: 'bg-green-100 dark:bg-green-900/30', text: 'text-green-700 dark:text-green-400', label: 'Personal' },
  general: { bg: 'bg-zinc-100 dark:bg-zinc-800', text: 'text-zinc-600 dark:text-zinc-400', label: 'General' },
  notification: { bg: 'bg-sky-100 dark:bg-sky-900/30', text: 'text-sky-700 dark:text-sky-400', label: 'Notification' },
  promotion: { bg: 'bg-orange-100 dark:bg-orange-900/30', text: 'text-orange-700 dark:text-orange-400', label: 'Promotion' },
  newsletter: { bg: 'bg-red-100 dark:bg-red-900/30', text: 'text-red-700 dark:text-red-400', label: 'Newsletter' },
  receipt: { bg: 'bg-emerald-100 dark:bg-emerald-900/30', text: 'text-emerald-700 dark:text-emerald-400', label: 'Receipt' },
  shipping: { bg: 'bg-teal-100 dark:bg-teal-900/30', text: 'text-teal-700 dark:text-teal-400', label: 'Shipping' },
  travel: { bg: 'bg-cyan-100 dark:bg-cyan-900/30', text: 'text-cyan-700 dark:text-cyan-400', label: 'Travel' },
  finance: { bg: 'bg-yellow-100 dark:bg-yellow-900/30', text: 'text-yellow-700 dark:text-yellow-400', label: 'Finance' },
  work: { bg: 'bg-blue-100 dark:bg-blue-900/30', text: 'text-blue-700 dark:text-blue-400', label: 'Work' },
  spam: { bg: 'bg-amber-100 dark:bg-amber-900/30', text: 'text-amber-700 dark:text-amber-400', label: 'Spam' },
  scam: { bg: 'bg-red-100 dark:bg-red-900/30', text: 'text-red-700 dark:text-red-400', label: 'Scam' },
}

export function CategoryBadge({ category }: { category: string }) {
  const s = styles[category] ?? { bg: 'bg-zinc-100 dark:bg-zinc-800', text: 'text-zinc-500 dark:text-zinc-400', label: category }
  return (
    <span className={`inline-flex items-center rounded-full px-1.5 py-0.5 text-[11px] font-medium capitalize ${s.bg} ${s.text}`}>
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
  critical: { bg: 'bg-red-100 dark:bg-red-900/30', text: 'text-red-700 dark:text-red-400', label: 'Critical', icon: '!!' },
  important: { bg: 'bg-amber-100 dark:bg-amber-900/30', text: 'text-amber-700 dark:text-amber-400', label: 'Important', icon: '!' },
  normal: { bg: 'bg-zinc-100 dark:bg-zinc-800', text: 'text-zinc-500 dark:text-zinc-400', label: 'Normal', icon: '' },
  low: { bg: 'bg-zinc-50 dark:bg-zinc-900', text: 'text-zinc-400 dark:text-zinc-500', label: 'Low', icon: '' },
  noise: { bg: 'bg-zinc-50 dark:bg-zinc-900', text: 'text-zinc-300 dark:text-zinc-600', label: 'Noise', icon: '' },
}

export function ImportanceBadge({ level }: { level: string }) {
  if (!level || level === 'normal') return null
  const s = importanceStyles[level] ?? importanceStyles.normal
  return (
    <span className={`inline-flex items-center gap-0.5 rounded-full px-1.5 py-0.5 text-[11px] font-medium ${s.bg} ${s.text}`}>
      {s.icon && <span className="font-bold">{s.icon}</span>}
      {s.label}
    </span>
  )
}

export function ActionBadge() {
  return (
    <span className="inline-flex items-center gap-0.5 rounded-full bg-purple-100 px-1.5 py-0.5 text-[11px] font-medium text-purple-700 dark:bg-purple-900/30 dark:text-purple-400">
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
    alert: { bg: 'bg-red-100 dark:bg-red-900/30', text: 'text-red-700 dark:text-red-400' },
  }
  const s = intentStyles[intent] ?? { bg: 'bg-zinc-100 dark:bg-zinc-800', text: 'text-zinc-500 dark:text-zinc-400' }
  return (
    <span className={`inline-flex items-center rounded-full px-1.5 py-0.5 text-[11px] font-medium capitalize ${s.bg} ${s.text}`}>
      {intent}
    </span>
  )
}
