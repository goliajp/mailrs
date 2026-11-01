import { type Key, type Translate, useT } from '@/i18n'
const styles: Record<string, { bg: string; label: Key; text: string }> = {
  finance: {
    bg: 'bg-warning/10',
    label: 'category.finance',
    text: 'text-warning',
  },
  general: {
    bg: 'bg-border-strong',
    label: 'category.general',
    text: 'text-fg',
  },
  // a row is `surface`, and in the dark theme `bg-tertiary` is the same
  // colour; a read row is also drawn at 70%, so the chip takes the
  // strongest neutral fill and text
  inbox: {
    bg: 'bg-border-strong',
    label: 'category.inbox',
    text: 'text-fg',
  },
  newsletter: {
    bg: 'bg-danger/10',
    label: 'category.newsletter',
    text: 'text-danger',
  },
  notification: {
    bg: 'bg-accent/10',
    label: 'category.notification',
    text: 'text-accent',
  },
  personal: {
    bg: 'bg-success/10',
    label: 'category.personal',
    text: 'text-success',
  },
  promotion: {
    bg: 'bg-warning/10',
    label: 'category.promotion',
    text: 'text-warning',
  },
  receipt: {
    bg: 'bg-success/10',
    label: 'category.receipt',
    text: 'text-success',
  },
  // v2.4.2 Phase 4.3 — terminology sweep: the category axis keeps
  // its internal `'spam'` / `'scam'` values (kevy key names,
  // decision.rs thresholds, backfill scripts all reference them by
  // these strings), but the user-visible label converges on "Junk"
  // so we match the Junk top-level folder introduced in Phase 2.
  // The finer classifier signal stays in the wire, but doesn't
  // split the UI.
  scam: {
    bg: 'bg-danger/10',
    label: 'list.junk',
    text: 'text-danger',
  },
  shipping: {
    bg: 'bg-info/10',
    label: 'category.shipping',
    text: 'text-info',
  },
  spam: {
    bg: 'bg-warning/10',
    label: 'list.junk',
    text: 'text-warning',
  },
  travel: {
    bg: 'bg-info/10',
    label: 'category.travel',
    text: 'text-info',
  },
  work: {
    bg: 'bg-accent/10',
    label: 'category.work',
    text: 'text-accent',
  },
}

export function CategoryBadge({ category }: { category: string }) {
  const t = useT()
  const s = styles[category]
  const tone = s ?? { bg: 'bg-border-strong', text: 'text-fg' }
  return (
    <span
      className={`inline-flex items-center rounded-full px-1.5 py-0.5 text-xs font-medium capitalize select-none ${tone.bg} ${tone.text}`}
    >
      {categoryName(s, category, t)}
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

const importanceStyles: Record<string, { bg: string; icon: string; label: Key; text: string }> = {
  critical: {
    bg: 'bg-danger/10',
    icon: '!!',
    label: 'importance.critical',
    text: 'text-danger',
  },
  important: {
    bg: 'bg-warning/10',
    icon: '!',
    label: 'importance.important',
    text: 'text-warning',
  },
  low: {
    bg: 'bg-bg-secondary',
    icon: '',
    label: 'importance.low',
    text: 'text-fg-muted',
  },
  noise: {
    bg: 'bg-bg-secondary',
    icon: '',
    label: 'importance.noise',
    text: 'text-fg-muted',
  },
  normal: {
    bg: 'bg-surface',
    icon: '',
    label: 'importance.normal',
    text: 'text-fg-muted',
  },
}

export function ActionBadge() {
  const t = useT()
  return (
    <span className="bg-accent/10 text-accent inline-flex items-center gap-0.5 rounded-full px-1.5 py-0.5 text-xs font-medium select-none">
      {t('badge.action')}
    </span>
  )
}

export function ImportanceBadge({ level }: { level: string }) {
  const t = useT()
  if (!level || level === 'normal' || level === 'low' || level === 'noise') return null
  const s = importanceStyles[level] ?? importanceStyles.normal
  return (
    <span
      className={`inline-flex items-center gap-0.5 rounded-full px-1.5 py-0.5 text-xs font-medium select-none ${s.bg} ${s.text}`}
    >
      {s.icon && <span className="font-bold">{s.icon}</span>}
      {t(s.label)}
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

/** A category we draw by its name; one we do not know, as the server sent it. */
function categoryName(style: undefined | { label: Key }, category: string, t: Translate): string {
  if (style) return t(style.label)
  return category
}
