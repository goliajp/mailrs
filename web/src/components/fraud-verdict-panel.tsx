/**
 * Why this conversation was held, in the four layers that decided it.
 *
 * The wave this was built for **passed every authentication check** —
 * all 25 impersonating messages passed SPF, 18 passed DKIM and DMARC.
 * So a panel that only showed a padlock would have shown a green one.
 * The useful sentence is the whole of it: transport says yes, the name
 * is a lie, and the mailer is one no client writes.
 *
 * Nothing here recomputes. It renders the verdict stored when the mail
 * arrived, with the rule-set version beside it, because what a reader
 * needs to know is what was decided then.
 */

import type { WireFraudLayer } from '@/wire/schemas/fraud'
import type { LucideIcon } from 'lucide-react'

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { CircleCheck, CircleMinus, ShieldAlert, TriangleAlert } from 'lucide-react'

import { conversationKeys } from '@/store/query-keys-v21'
import { fetchFraudVerdict, releaseQuarantined } from '@/wire/endpoints/fraud'

/** What each layer was actually asking. */
const LAYER_QUESTION: Record<string, string> = {
  content: 'Does it read like the fraud it is?',
  identity: 'Is the name it shows a claim it can make?',
  provenance: 'Did a real mail client write it?',
  transport: 'Did it come from where it says?',
}

const LAYER_TITLE: Record<string, string> = {
  content: 'Content',
  identity: 'Identity',
  provenance: 'Provenance',
  transport: 'Transport',
}

export function FraudVerdictPanel({
  messageId,
  threadId,
}: {
  messageId: string
  threadId: string
}) {
  const qc = useQueryClient()
  const { data: verdict } = useQuery({
    enabled: messageId !== '',
    // The verdict never changes after it is written, so this is as
    // static as data gets.
    queryKey: ['fraud-verdict', messageId],
    staleTime: Infinity,
    queryFn: ({ signal }) => fetchFraudVerdict(messageId, signal),
  })
  const release = useMutation({
    mutationFn: () => releaseQuarantined(threadId),
    onSuccess: () => qc.invalidateQueries({ queryKey: conversationKeys.all() }),
  })

  // Almost every message. Rendering nothing is right: a message nobody
  // suspected has no finding, and an "examined, nothing found" banner
  // on every mail would make the ones that matter invisible.
  if (!verdict) return null

  return (
    <div className="border-border bg-bg-secondary mx-4 mb-2 rounded-lg border">
      <div className="flex items-start gap-2 px-3 pt-2.5 pb-1.5">
        <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0 text-red-500" />
        <div className="min-w-0 flex-1">
          <p className="text-fg text-xs font-semibold">
            {verdict.quarantined ? 'Held: suspected fraud' : 'Examined: something looked wrong'}
          </p>
          <p className="text-fg-muted text-[11px] leading-snug">
            {/* The layers below name the check that convicted.
                Repeating a number here invited the reader to do the
                arithmetic instead — and on production the arithmetic
                disagreed with the hold on 43 of 51 conversations. */}
            Kept, not deleted — this is the evidence an abuse report is built from.
          </p>
        </div>
      </div>

      <ul className="border-border border-t px-3 py-1">
        {verdict.layers.map((l) => (
          <Layer key={l.name} layer={l} />
        ))}
      </ul>

      <div className="border-border flex items-center justify-between gap-2 border-t px-3 py-1.5">
        <span className="text-fg-muted font-mono text-[10px]">rules {verdict.rules_version}</span>
        {verdict.quarantined && (
          <button
            className="border-border text-fg hover:bg-bg rounded-md border px-2 py-1 text-[11px] font-medium disabled:opacity-50"
            disabled={release.isPending}
            onClick={() => release.mutate()}
          >
            {release.isPending ? 'Releasing…' : 'This is not fraud'}
          </button>
        )}
      </div>
    </div>
  )
}

function Layer({ layer }: { layer: WireFraudLayer }) {
  const Icon = outcomeIcon(layer.outcome)
  return (
    <li className="flex items-start gap-2 py-1">
      <Icon className={`mt-0.5 h-3.5 w-3.5 shrink-0 ${outcomeClass(layer.outcome)}`} />
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline justify-between gap-2">
          <span className="text-fg text-xs font-medium">
            {LAYER_TITLE[layer.name] ?? layer.name}
          </span>
          {layer.score > 0 && (
            <span className="text-fg-muted shrink-0 font-mono text-[10px]">
              +{layer.score.toFixed(1)}
            </span>
          )}
        </div>
        <p className="text-fg-muted text-[11px] leading-snug">{LAYER_QUESTION[layer.name]}</p>
        <p className="text-fg-secondary text-[11px] leading-snug break-words">{layer.detail}</p>
      </div>
    </li>
  )
}

function outcomeClass(outcome: WireFraudLayer['outcome']): string {
  switch (outcome) {
    case 'fail':
      return 'text-red-500'
    case 'not_applicable':
      return 'text-fg-muted'
    case 'pass':
      return 'text-emerald-500'
  }
}

function outcomeIcon(outcome: WireFraudLayer['outcome']): LucideIcon {
  switch (outcome) {
    case 'fail':
      return TriangleAlert
    case 'not_applicable':
      // Not a tick. "Nothing was checked" and "everything was fine"
      // are different facts, and a green mark for the first one tells
      // the reader something untrue about mail nobody vouched for.
      return CircleMinus
    case 'pass':
      return CircleCheck
  }
}
