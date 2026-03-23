const styles: Record<string, { bg: string; label: string; text: string }> = {
  finance: {
    bg: 'bg-[var(--color-status-warning-subtle)]',
    label: 'Finance',
    text: 'text-[var(--color-status-warning)]',
  },
  general: {
    bg: 'bg-[var(--color-bg-raised)]',
    label: 'General',
    text: 'text-[var(--color-text-secondary)]',
  },
  newsletter: {
    bg: 'bg-[var(--color-status-danger-subtle)]',
    label: 'Newsletter',
    text: 'text-[var(--color-status-danger)]',
  },
  notification: {
    bg: 'bg-[var(--color-brand-subtle)]',
    label: 'Notification',
    text: 'text-[var(--color-brand-primary)]',
  },
  personal: {
    bg: 'bg-[var(--color-status-success-subtle)]',
    label: 'Personal',
    text: 'text-[var(--color-status-success)]',
  },
  promotion: {
    bg: 'bg-[var(--color-status-warning-subtle)]',
    label: 'Promotion',
    text: 'text-[var(--color-status-warning)]',
  },
  receipt: {
    bg: 'bg-[var(--color-status-success-subtle)]',
    label: 'Receipt',
    text: 'text-[var(--color-status-success)]',
  },
  scam: {
    bg: 'bg-[var(--color-status-danger-subtle)]',
    label: 'Scam',
    text: 'text-[var(--color-status-danger)]',
  },
  shipping: {
    bg: 'bg-[var(--color-status-info-subtle)]',
    label: 'Shipping',
    text: 'text-[var(--color-status-info)]',
  },
  spam: {
    bg: 'bg-[var(--color-status-warning-subtle)]',
    label: 'Spam',
    text: 'text-[var(--color-status-warning)]',
  },
  travel: {
    bg: 'bg-[var(--color-status-info-subtle)]',
    label: 'Travel',
    text: 'text-[var(--color-status-info)]',
  },
  work: {
    bg: 'bg-[var(--color-brand-subtle)]',
    label: 'Work',
    text: 'text-[var(--color-brand-primary)]',
  },
}

export function CategoryBadge({ category }: { category: string }) {
  const s = styles[category] ?? {
    bg: 'bg-[var(--color-bg-raised)]',
    label: category,
    text: 'text-[var(--color-text-tertiary)]',
  }
  return (
    <span
      className={`inline-flex items-center rounded-full px-1.5 py-0.5 text-xs font-medium capitalize select-none ${s.bg} ${s.text}`}
    >
      {s.label}
    </span>
  )
}

// eslint-disable-next-line react-refresh/only-export-components
export function riskColor(score: number): string {
  if (score >= 60) return 'text-[var(--color-status-danger)]'
  if (score >= 40) return 'text-[var(--color-status-warning)]'
  if (score >= 15) return 'text-[var(--color-status-info)]'
  return 'text-[var(--color-status-success)]'
}

const importanceStyles: Record<
  string,
  { bg: string; icon: string; label: string; text: string }
> = {
  critical: {
    bg: 'bg-[var(--color-status-danger-subtle)]',
    icon: '!!',
    label: 'Critical',
    text: 'text-[var(--color-status-danger)]',
  },
  important: {
    bg: 'bg-[var(--color-status-warning-subtle)]',
    icon: '!',
    label: 'Important',
    text: 'text-[var(--color-status-warning)]',
  },
  low: {
    bg: 'bg-[var(--color-bg-sunken)]',
    icon: '',
    label: 'Low',
    text: 'text-[var(--color-text-tertiary)]',
  },
  noise: {
    bg: 'bg-[var(--color-bg-sunken)]',
    icon: '',
    label: 'Noise',
    text: 'text-[var(--color-text-tertiary)]',
  },
  normal: {
    bg: 'bg-[var(--color-bg-raised)]',
    icon: '',
    label: 'Normal',
    text: 'text-[var(--color-text-tertiary)]',
  },
}

export function ActionBadge() {
  return (
    <span className="inline-flex items-center gap-0.5 rounded-full bg-[var(--color-brand-subtle)] px-1.5 py-0.5 text-xs font-medium text-[var(--color-brand-primary)] select-none">
      Action
    </span>
  )
}

export function ImportanceBadge({ level }: { level: string }) {
  if (!level || level === 'normal' || level === 'low' || level === 'noise')
    return null
  const s = importanceStyles[level] ?? importanceStyles.normal
  return (
    <span
      className={`inline-flex items-center gap-0.5 rounded-full px-1.5 py-0.5 text-xs font-medium select-none ${s.bg} ${s.text}`}
    >
      {s.icon && <span className="font-bold">{s.icon}</span>}
      {s.label}
    </span>
  )
}

export function IntentBadge({ intent }: { intent: string }) {
  if (!intent || intent === 'inform') return null
  const intentStyles: Record<string, { bg: string; text: string }> = {
    alert: {
      bg: 'bg-[var(--color-status-danger-subtle)]',
      text: 'text-[var(--color-status-danger)]',
    },
    confirm: {
      bg: 'bg-[var(--color-status-success-subtle)]',
      text: 'text-[var(--color-status-success)]',
    },
    request: {
      bg: 'bg-[var(--color-brand-subtle)]',
      text: 'text-[var(--color-brand-primary)]',
    },
    social: {
      bg: 'bg-[var(--color-status-info-subtle)]',
      text: 'text-[var(--color-status-info)]',
    },
  }
  const s = intentStyles[intent] ?? {
    bg: 'bg-[var(--color-bg-raised)]',
    text: 'text-[var(--color-text-tertiary)]',
  }
  return (
    <span
      className={`inline-flex items-center rounded-full px-1.5 py-0.5 text-xs font-medium capitalize select-none ${s.bg} ${s.text}`}
    >
      {intent}
    </span>
  )
}
