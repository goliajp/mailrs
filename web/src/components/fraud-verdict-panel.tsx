import type { WireFraudLayer, WireFraudVerdict } from '@/wire/schemas/fraud'
import type { WireReason } from '@/wire/schemas/reason'
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
 *
 * Every reason is a code the server stored; the words come from the
 * shared reasons table in the reader's language. A code this build has
 * no sentence for falls back to the server's English detail.
 */
import type { LucideIcon } from 'lucide-react'

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useAtomValue } from 'jotai'
import { CircleCheck, CircleMinus, Info, ShieldAlert, TriangleAlert } from 'lucide-react'

import { either, type Key, type Lang, langAtom, type Translate, useT } from '@/i18n'
import { hasReasonText, reasonText } from '@/lib/reasons'
import { conversationKeys } from '@/store/query-keys-v21'
import { fetchFraudVerdict, releaseQuarantined } from '@/wire/endpoints/fraud'

/** Each layer's name, and what it was actually asking. */
const LAYER: Record<string, undefined | { question: Key; title: Key }> = {
  content: { question: 'fraud.layer.content.question', title: 'fraud.layer.content' },
  identity: { question: 'fraud.layer.identity.question', title: 'fraud.layer.identity' },
  provenance: { question: 'fraud.layer.provenance.question', title: 'fraud.layer.provenance' },
  transport: { question: 'fraud.layer.transport.question', title: 'fraud.layer.transport' },
}

export function FraudVerdictPanel({
  messageId,
  threadId,
}: {
  messageId: string
  threadId: string
}) {
  const t = useT()
  const qc = useQueryClient()
  const { data: verdict } = useQuery({
    enabled: messageId !== '',
    // It does change. `maintenance:fraud-rescan` rewrites the verdict
    // on every run — a new `rules_version` and a new set of reasons —
    // and the rules moved several times on 2026-08-30 alone. The
    // comment here used to say it never changes after it is written,
    // which is how a reader would have gone on being shown the reasons
    // a since-retired rule gave.
    queryKey: ['fraud-verdict', messageId],
    staleTime: 60_000,
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
  // mail nobody suspected, sorted by a rule, the classifier or a filter
  if (verdict.layers.length === 0) return <PlacementNote reasons={verdict.placement} />

  return (
    <div className="border-border bg-bg-secondary mx-4 mb-2 rounded-lg border">
      <div className="flex items-start gap-2 px-3 pt-2.5 pb-1.5">
        <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0 text-red-500" />
        <div className="min-w-0 flex-1">
          <p className="text-fg text-xs font-semibold">
            {t(either(verdict.quarantined, 'list.held', 'fraud.examined'))}
          </p>
          <p className="text-fg-muted text-[11px] leading-snug">
            {/* The layers below name the check that convicted.
                Repeating a number here invited the reader to do the
                arithmetic instead — and on production the arithmetic
                disagreed with the hold on 43 of 51 conversations. */}
            {t('fraud.kept')}
          </p>
        </div>
      </div>

      <Placement verdict={verdict} />
      <ul className="border-border border-t px-3 py-1">
        {verdict.layers.map((l) => (
          <Layer key={l.name} layer={l} />
        ))}
      </ul>

      <div className="border-border flex items-center justify-between gap-2 border-t px-3 py-1.5">
        <span className="text-fg-muted font-mono text-[10px]">
          {t('fraud.rules', { version: verdict.rules_version })}
        </span>
        {verdict.quarantined && (
          <button
            className="border-border text-fg hover:bg-bg rounded-md border px-2 py-1 text-[11px] font-medium disabled:opacity-50"
            disabled={release.isPending}
            onClick={() => release.mutate()}
          >
            {t(either(release.isPending, 'fraud.releasing', 'fraud.notFraud'))}
          </button>
        )}
      </div>
    </div>
  )
}

function Layer({ layer }: { layer: WireFraudLayer }) {
  const t = useT()
  const lang = useAtomValue(langAtom)
  const Icon = outcomeIcon(layer.outcome)
  const question = LAYER[layer.name]?.question
  return (
    <li className="flex items-start gap-2 py-1" data-testid={`verdict-layer-${layer.name}`}>
      <Icon className={`mt-0.5 h-3.5 w-3.5 shrink-0 ${outcomeClass(layer.outcome)}`} />
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline justify-between gap-2">
          <span className="text-fg text-xs font-medium">{layerTitle(layer.name, t)}</span>
          {layer.score > 0 && (
            <span
              className="text-fg-muted shrink-0 font-mono text-[10px]"
              data-testid="layer-score"
            >
              +{layer.score.toFixed(1)}
            </span>
          )}
        </div>
        {question && <p className="text-fg-muted text-[11px] leading-snug">{t(question)}</p>}
        <LayerReasons lang={lang} layer={layer} t={t} />
      </div>
    </li>
  )
}

/**
 * What the layer saw, in the reader's language. The server's English
 * detail stands in when the verdict predates reasons or carries a code
 * this build has no sentence for.
 */
function LayerReasons({ lang, layer, t }: { lang: Lang; layer: WireFraudLayer; t: Translate }) {
  if (layer.reasons.length === 0 && layer.outcome === 'pass') {
    return <p className="text-fg-secondary text-[11px] leading-snug">{t('fraud.layer.clean')}</p>
  }
  const worded = layer.reasons.length > 0 && layer.reasons.every((r) => hasReasonText(r.code))
  if (!worded) {
    return <p className="text-fg-secondary text-[11px] leading-snug break-words">{layer.detail}</p>
  }
  return <ReasonList lang={lang} reasons={layer.reasons} t={t} />
}

function layerTitle(name: string, t: Translate): string {
  const key = LAYER[name]?.title
  if (key) return t(key)
  return name
}

/**
 * What one line added to its layer, beside it, so the layer's score is
 * the sum of the numbers the reader sees under it.
 */
function LineScore({ score }: { score: number | undefined }) {
  if (score === undefined || score <= 0) return null
  return (
    <span className="text-fg-muted shrink-0 font-mono text-[10px]" data-testid="line-score">
      +{score.toFixed(1)}
    </span>
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

/** Why it was filed where it is, beside the four layers. */
function Placement({ verdict }: { verdict: WireFraudVerdict }) {
  const t = useT()
  const lang = useAtomValue(langAtom)
  if (verdict.placement.length === 0) return null
  return (
    <div className="border-border border-t px-3 py-1.5">
      <p className="text-fg text-xs font-medium">{t('fraud.placement')}</p>
      <ReasonList lang={lang} reasons={verdict.placement} t={t} />
    </div>
  )
}

/** The whole panel, for mail no layer found anything in. */
function PlacementNote({ reasons }: { reasons: readonly WireReason[] }) {
  const t = useT()
  const lang = useAtomValue(langAtom)
  if (reasons.length === 0) return null
  return (
    <div
      className="border-border bg-bg-secondary mx-4 mb-2 flex items-start gap-2 rounded-lg border px-3 py-2"
      data-testid="placement-note"
    >
      <Info className="text-fg-muted mt-0.5 h-3.5 w-3.5 shrink-0" />
      <div className="min-w-0 flex-1">
        <p className="text-fg text-xs font-medium">{t('fraud.placement')}</p>
        <ReasonList lang={lang} reasons={reasons} t={t} />
      </div>
    </div>
  )
}

function ReasonList({
  lang,
  reasons,
  t,
}: {
  lang: Lang
  reasons: readonly WireReason[]
  t: Translate
}) {
  return (
    <ul className="text-fg-secondary text-[11px] leading-snug">
      {reasons.map((r, i) => (
        // static per verdict: the stored list never reorders
        <li className="flex items-baseline justify-between gap-2" key={`${r.code}-${i}`}>
          <span className="min-w-0 break-words">{reasonText(r, lang, t) ?? r.code}</span>
          <LineScore score={r.score} />
        </li>
      ))}
    </ul>
  )
}
