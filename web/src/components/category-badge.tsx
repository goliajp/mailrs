const styles: Record<string, { bg: string; text: string; label: string }> = {
  personal: {
    bg: 'bg-[var(--color-status-success-subtle)]',
    text: 'text-[var(--color-status-success)]',
    label: 'Personal',
  },
  general: {
    bg: 'bg-[var(--color-bg-raised)]',
    text: 'text-[var(--color-text-secondary)]',
    label: 'General',
  },
  notification: {
    bg: 'bg-[var(--color-brand-subtle)]',
    text: 'text-[var(--color-brand-primary)]',
    label: 'Notification',
  },
  promotion: {
    bg: 'bg-[var(--color-status-warning-subtle)]',
    text: 'text-[var(--color-status-warning)]',
    label: 'Promotion',
  },
  newsletter: {
    bg: 'bg-[var(--color-status-danger-subtle)]',
    text: 'text-[var(--color-status-danger)]',
    label: 'Newsletter',
  },
  receipt: {
    bg: 'bg-[var(--color-status-success-subtle)]',
    text: 'text-[var(--color-status-success)]',
    label: 'Receipt',
  },
  shipping: {
    bg: 'bg-[var(--color-status-info-subtle)]',
    text: 'text-[var(--color-status-info)]',
    label: 'Shipping',
  },
  travel: {
    bg: 'bg-[var(--color-status-info-subtle)]',
    text: 'text-[var(--color-status-info)]',
    label: 'Travel',
  },
  finance: {
    bg: 'bg-[var(--color-status-warning-subtle)]',
    text: 'text-[var(--color-status-warning)]',
    label: 'Finance',
  },
  work: {
    bg: 'bg-[var(--color-brand-subtle)]',
    text: 'text-[var(--color-brand-primary)]',
    label: 'Work',
  },
  spam: {
    bg: 'bg-[var(--color-status-warning-subtle)]',
    text: 'text-[var(--color-status-warning)]',
    label: 'Spam',
  },
  scam: {
    bg: 'bg-[var(--color-status-danger-subtle)]',
    text: 'text-[var(--color-status-danger)]',
    label: 'Scam',
  },
}

export function CategoryBadge({ category }: { category: string }) {
  const s = styles[category] ?? {
    bg: 'bg-[var(--color-bg-raised)]',
    text: 'text-[var(--color-text-tertiary)]',
    label: category,
  }
  return (
    <span
      className={`inline-flex select-none items-center px-1.5 py-0.5 text-xs font-medium rounded-full capitalize ${s.bg} ${s.text}`}
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

const importanceStyles: Record<string, { bg: string; text: string; label: string; icon: string }> =
  {
    critical: {
      bg: 'bg-[var(--color-status-danger-subtle)]',
      text: 'text-[var(--color-status-danger)]',
      label: 'Critical',
      icon: '!!',
    },
    important: {
      bg: 'bg-[var(--color-status-warning-subtle)]',
      text: 'text-[var(--color-status-warning)]',
      label: 'Important',
      icon: '!',
    },
    normal: {
      bg: 'bg-[var(--color-bg-raised)]',
      text: 'text-[var(--color-text-tertiary)]',
      label: 'Normal',
      icon: '',
    },
    low: {
      bg: 'bg-[var(--color-bg-sunken)]',
      text: 'text-[var(--color-text-tertiary)]',
      label: 'Low',
      icon: '',
    },
    noise: {
      bg: 'bg-[var(--color-bg-sunken)]',
      text: 'text-[var(--color-text-tertiary)]',
      label: 'Noise',
      icon: '',
    },
  }

export function ImportanceBadge({ level }: { level: string }) {
  if (!level || level === 'normal' || level === 'low' || level === 'noise') return null
  const s = importanceStyles[level] ?? importanceStyles.normal
  return (
    <span
      className={`inline-flex select-none items-center gap-0.5 px-1.5 py-0.5 text-xs font-medium rounded-full ${s.bg} ${s.text}`}
    >
      {s.icon && <span className="font-bold">{s.icon}</span>}
      {s.label}
    </span>
  )
}

export function ActionBadge() {
  return (
    <span className="inline-flex select-none items-center gap-0.5 px-1.5 py-0.5 text-xs font-medium rounded-full bg-[var(--color-brand-subtle)] text-[var(--color-brand-primary)]">
      Action
    </span>
  )
}

export function IntentBadge({ intent }: { intent: string }) {
  if (!intent || intent === 'inform') return null
  const intentStyles: Record<string, { bg: string; text: string }> = {
    request: { bg: 'bg-[var(--color-brand-subtle)]', text: 'text-[var(--color-brand-primary)]' },
    confirm: {
      bg: 'bg-[var(--color-status-success-subtle)]',
      text: 'text-[var(--color-status-success)]',
    },
    social: { bg: 'bg-[var(--color-status-info-subtle)]', text: 'text-[var(--color-status-info)]' },
    alert: {
      bg: 'bg-[var(--color-status-danger-subtle)]',
      text: 'text-[var(--color-status-danger)]',
    },
  }
  const s = intentStyles[intent] ?? {
    bg: 'bg-[var(--color-bg-raised)]',
    text: 'text-[var(--color-text-tertiary)]',
  }
  return (
    <span
      className={`inline-flex select-none items-center px-1.5 py-0.5 text-xs font-medium rounded-full capitalize ${s.bg} ${s.text}`}
    >
      {intent}
    </span>
  )
}
