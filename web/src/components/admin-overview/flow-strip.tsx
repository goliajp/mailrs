import type { Lane, Stage, Tone } from './flow-model'
import type { AdminFlow } from '@/wire/schemas/admin-flow'

import { ChevronRight } from 'lucide-react'
import { Link } from 'react-router'

import { cn } from '@/lib/cn'

import { lanesOf } from './flow-model'
import { count } from './format'

/**
 * Where mail is in the server right now, one lane per direction, left to
 * right in the order a message moves. A step whose numbers could not be
 * read shows a dash, never a zero.
 */
export function FlowStrip({ flow }: { flow: AdminFlow }) {
  return (
    <div
      aria-label="Mail flow"
      className="border-border bg-surface divide-border divide-y rounded-lg border"
      role="group"
    >
      {lanesOf(flow).map((lane) => (
        <LaneRow key={lane.name} lane={lane} />
      ))}
    </div>
  )
}

function Connector() {
  return (
    <div aria-hidden className="text-fg-muted flex items-center justify-center">
      <ChevronRight className="h-4 w-4 rotate-90 md:rotate-0" />
    </div>
  )
}

function LaneRow({ lane }: { lane: Lane }) {
  return (
    <section aria-label={lane.name} className="flex flex-col lg:flex-row">
      <header className="border-border px-4 pt-4 lg:w-40 lg:shrink-0 lg:border-r lg:py-4">
        <h3 className="text-fg text-sm font-medium">{lane.name}</h3>
        <p className="text-fg-muted text-xs">{lane.note}</p>
      </header>
      <ol aria-label={`${lane.name} steps`} className="flex min-w-0 flex-1 flex-col md:flex-row">
        {lane.stages.map((stage, i) => (
          <li className="flex min-w-0 flex-1 flex-col md:flex-row" key={stage.name}>
            <StageCell stage={stage} />
            {i < lane.stages.length - 1 && <Connector />}
          </li>
        ))}
      </ol>
    </section>
  )
}

function StageCell({ stage }: { stage: Stage }) {
  const body = (
    <div className="min-w-0 flex-1 px-4 py-4">
      <p className="text-fg-secondary text-sm">{stage.name}</p>
      <p className={cn('mt-1 text-3xl font-semibold tabular-nums', toneClass(stage.tone))}>
        {count(stage.value)}
      </p>
      <p className="text-fg-muted text-xs">{stage.caption}</p>
      <ul className="text-fg-muted mt-2 space-y-0.5 text-xs">
        {stage.details.map((d) => (
          <li key={d}>{d}</li>
        ))}
      </ul>
    </div>
  )
  if (stage.to === undefined) return body
  return (
    <Link
      className="hover:bg-bg-secondary focus-visible:ring-accent flex min-w-0 flex-1 transition-colors focus-visible:ring-2 focus-visible:outline-none"
      to={stage.to}
    >
      {body}
    </Link>
  )
}

function toneClass(tone: Tone): string {
  if (tone === 'danger') return 'text-danger'
  if (tone === 'warning') return 'text-warning'
  return 'text-fg'
}
