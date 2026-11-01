import type { WireRule } from '@/wire/schemas/rules'

import { toast } from '@goliapkg/gds'
import { useState } from 'react'
import { useSearchParams } from 'react-router'

import { useLabels } from '@/hooks/use-labels'
import { useApplyRule, useDeleteRule, useRules, useSaveRule } from '@/hooks/use-rules'
import { useT } from '@/i18n'
import { wirePreviewRule } from '@/wire/endpoints/rules'

import {
  btnPrimary,
  btnSecondary,
  cardClass,
  ConfirmDialog,
  SectionHeader,
  Toggle,
} from './_shared'
import { RuleForm } from './rule-form'
import { errorText, PLAIN_ACTIONS, runToast } from './rules-shared'

/**
 * Rules: a search, and what to do with the mail it finds. They run on mail
 * as it arrives; "Apply to existing mail" runs one over what is already
 * here, and the toast after it undoes that.
 */
export function RulesSection() {
  const t = useT()
  const [params, setParams] = useSearchParams()
  const fromSearch = params.get('rule_query') ?? ''
  const [adding, setAdding] = useState(fromSearch !== '')
  const { data: rules } = useRules()
  const done = () => {
    setAdding(false)
    setParams((prev) => {
      const next = new URLSearchParams(prev)
      next.delete('rule_query')
      return next
    })
  }
  return (
    <div className="space-y-4">
      <SectionHeader title={t('settings.cat.rules')} />
      <p className="text-fg-muted text-xs">{t('settings.rules.help')}</p>
      {adding && <RuleForm initialQuery={fromSearch} onDone={done} />}
      {!adding && (
        <button className={btnPrimary} onClick={() => setAdding(true)} type="button">
          {t('settings.rules.new')}
        </button>
      )}
      {(rules ?? []).map((r) => (
        <RuleRow key={r.id} rule={r} />
      ))}
      {rules && rules.length === 0 && !adding && (
        <p className="text-fg-muted text-sm">{t('settings.rules.empty')}</p>
      )}
    </div>
  )
}

function RuleRow({ rule }: { rule: WireRule }) {
  const t = useT()
  const { data: labels } = useLabels()
  const save = useSaveRule()
  const remove = useDeleteRule()
  const apply = useApplyRule()
  const [confirm, setConfirm] = useState<null | number>(null)
  const describe = (a: string) => {
    const plain = PLAIN_ACTIONS.find((p) => p.value === a)
    if (plain) return t(plain.label)
    const id = a.replace(/^label:/, '')
    const name = (labels ?? []).find((l) => l.id === id)?.name ?? t('settings.rules.deletedLabel')
    return t('settings.rules.labelAction', { name })
  }
  const askToRun = async () => {
    try {
      setConfirm((await wirePreviewRule(rule.query)).count)
    } catch (e) {
      toast.error(errorText(e))
    }
  }
  return (
    <div className={`${cardClass} space-y-2`}>
      <div className="flex items-center justify-between gap-3">
        <span className="font-medium">{rule.name}</span>
        <Toggle
          checked={rule.enabled}
          onChange={(enabled) =>
            save.mutate(
              { body: { ...rule, enabled }, id: rule.id },
              { onError: (e) => toast.error(errorText(e)) }
            )
          }
        />
      </div>
      <p className="text-fg-secondary font-mono text-xs break-all">{rule.query}</p>
      <p className="text-sm">{rule.actions.map(describe).join(', ')}</p>
      <div className="flex justify-end gap-2">
        <button className={btnSecondary} onClick={askToRun} type="button">
          {t('settings.rules.applyExisting')}
        </button>
        <button
          className={btnSecondary}
          onClick={() => remove.mutate(rule.id, { onError: (e) => toast.error(errorText(e)) })}
          type="button"
        >
          {t('settings.rules.delete')}
        </button>
      </div>
      {confirm !== null && (
        <ConfirmDialog
          message={t('settings.rules.applyConfirm', { count: confirm, name: rule.name })}
          onCancel={() => setConfirm(null)}
          onConfirm={() => {
            setConfirm(null)
            apply.mutate(rule.id, {
              onSuccess: runToast,
              onError: (e) => toast.error(errorText(e)),
            })
          }}
        />
      )}
    </div>
  )
}
