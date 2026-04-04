const styles: Record<string, { bg: string; label: string; text: string }> = {
  finance: {
    bg: 'bg-warning/10',
    label: 'Finance',
    text: 'text-warning',
  },
  general: {
    bg: 'bg-surface',
    label: 'General',
    text: 'text-fg-secondary',
  },
  newsletter: {
    bg: 'bg-danger/10',
    label: 'Newsletter',
    text: 'text-danger',
  },
  notification: {
    bg: 'bg-accent/10',
    label: 'Notification',
    text: 'text-accent',
  },
  personal: {
    bg: 'bg-success/10',
    label: 'Personal',
    text: 'text-success',
  },
  promotion: {
    bg: 'bg-warning/10',
    label: 'Promotion',
    text: 'text-warning',
  },
  receipt: {
    bg: 'bg-success/10',
    label: 'Receipt',
    text: 'text-success',
  },
  scam: {
    bg: 'bg-danger/10',
    label: 'Scam',
    text: 'text-danger',
  },
  shipping: {
    bg: 'bg-info/10',
    label: 'Shipping',
    text: 'text-info',
  },
  spam: {
    bg: 'bg-warning/10',
    label: 'Spam',
    text: 'text-warning',
  },
  travel: {
    bg: 'bg-info/10',
    label: 'Travel',
    text: 'text-info',
  },
  work: {
    bg: 'bg-accent/10',
    label: 'Work',
    text: 'text-accent',
  },
}

export function CategoryBadge({ category }: { category: string }) {
  const s = styles[category] ?? {
    bg: 'bg-surface',
    label: category,
    text: 'text-fg-muted',
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
  if (score >= 60) return 'text-danger'
  if (score >= 40) return 'text-warning'
  if (score >= 15) return 'text-info'
  return 'text-success'
}

const importanceStyles: Record<string, { bg: string; icon: string; label: string; text: string }> =
  {
    critical: {
      bg: 'bg-danger/10',
      icon: '!!',
      label: 'Critical',
      text: 'text-danger',
    },
    important: {
      bg: 'bg-warning/10',
      icon: '!',
      label: 'Important',
      text: 'text-warning',
    },
    low: {
      bg: 'bg-bg-secondary',
      icon: '',
      label: 'Low',
      text: 'text-fg-muted',
    },
    noise: {
      bg: 'bg-bg-secondary',
      icon: '',
      label: 'Noise',
      text: 'text-fg-muted',
    },
    normal: {
      bg: 'bg-surface',
      icon: '',
      label: 'Normal',
      text: 'text-fg-muted',
    },
  }

export function ActionBadge() {
  return (
    <span className="bg-accent/10 text-accent inline-flex items-center gap-0.5 rounded-full px-1.5 py-0.5 text-xs font-medium select-none">
      Action
    </span>
  )
}

export function ImportanceBadge({ level }: { level: string }) {
  if (!level || level === 'normal' || level === 'low' || level === 'noise') return null
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
      bg: 'bg-danger/10',
      text: 'text-danger',
    },
    confirm: {
      bg: 'bg-success/10',
      text: 'text-success',
    },
    request: {
      bg: 'bg-accent/10',
      text: 'text-accent',
    },
    social: {
      bg: 'bg-info/10',
      text: 'text-info',
    },
  }
  const s = intentStyles[intent] ?? {
    bg: 'bg-surface',
    text: 'text-fg-muted',
  }
  return (
    <span
      className={`inline-flex items-center rounded-full px-1.5 py-0.5 text-xs font-medium capitalize select-none ${s.bg} ${s.text}`}
    >
      {intent}
    </span>
  )
}
