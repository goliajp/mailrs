import { SectionHeader } from './_shared'
import { ForwardingCard } from './forwarding-card'
import { VacationCard } from './vacation-card'

/** Forwarding and the vacation reply: both act on mail as it arrives. */
export function ForwardingSection() {
  return (
    <div className="space-y-4">
      <SectionHeader title="Forwarding and vacation" />
      <ForwardingCard />
      <VacationCard />
    </div>
  )
}
