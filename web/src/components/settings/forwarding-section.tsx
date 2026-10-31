import { useT } from '@/i18n'

import { SectionHeader } from './_shared'
import { ForwardingCard } from './forwarding-card'
import { VacationCard } from './vacation-card'

/** Forwarding and the vacation reply: both act on mail as it arrives. */
export function ForwardingSection() {
  const t = useT()
  return (
    <div className="space-y-4">
      <SectionHeader title={t('settings.forwarding.title')} />
      <ForwardingCard />
      <VacationCard />
    </div>
  )
}
