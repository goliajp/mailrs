import type { WireRule } from '@/wire/schemas/rules'

import { toast } from '@goliapkg/gds'
import { useState } from 'react'
import { useSearchParams } from 'react-router'

import { useLabels } from '@/hooks/use-labels'
import { useApplyRule, useDeleteRule, useRules, useSaveRule } from '@/hooks/use-rules'
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
      <SectionHeader title="Rules" />
      <p className="text-fg-muted text-xs">
        Rules run on mail as it arrives, in the order they were made. A rule takes the same query as
        search, so its preview is what it will change. Mail held as suspected fraud is never changed
        by a rule, and mail in Junk is never taken out of it.
      </p>
      {adding && <RuleForm initialQuery={fromSearch} onDone={done} />}
      {!adding && (
        <button className={btnPrimary} onClick={() => setAdding(true)} type="button">
          New rule
        </button>
      )}
      {(rules ?? []).map((r) => (
        <RuleRow key={r.id} rule={r} />
      ))}
      {rules && rules.length === 0 && !adding && (
        <p className="text-fg-muted text-sm">No rules yet</p>
      )}
    </div>
  )
}

function RuleRow({ rule }: { rule: WireRule }) {
  const { data: labels } = useLabels()
  const save = useSaveRule()
  const remove = useDeleteRule()
  const apply = useApplyRule()
  const [confirm, setConfirm] = useState<null | number>(null)
  const describe = (a: string) => {
    const plain = PLAIN_ACTIONS.find((p) => p.value === a)
    if (plain) return plain.label
    const id = a.replace(/^label:/, '')
    const name = (labels ?? []).find((l) => l.id === id)?.name ?? 'a deleted label'
    return `Label ${name}`
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
          Apply to existing mail
        </button>
        <button
          className={btnSecondary}
          onClick={() => remove.mutate(rule.id, { onError: (e) => toast.error(errorText(e)) })}
          type="button"
        >
          Delete
        </button>
      </div>
      {confirm !== null && (
        <ConfirmDialog
          message={`Apply "${rule.name}" to the ${confirm} conversations that match now? You can undo it afterwards.`}
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
