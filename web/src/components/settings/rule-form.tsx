import { toast } from '@goliapkg/gds'
import { useState } from 'react'

import { useLabels } from '@/hooks/use-labels'
import { useApplyRule, useSaveRule } from '@/hooks/use-rules'
import { wirePreviewRule } from '@/wire/endpoints/rules'

import { btnPrimary, btnSecondary, cardClass, inputClass } from './_shared'
import { errorText, PLAIN_ACTIONS, runToast } from './rules-shared'

/** A new rule: the search, what to do, and how many it would change now. */
export function RuleForm({ initialQuery, onDone }: { initialQuery: string; onDone: () => void }) {
  const [name, setName] = useState('')
  const [query, setQuery] = useState(initialQuery)
  const [actions, setActions] = useState<string[]>([])
  const [label, setLabel] = useState('')
  const [preview, setPreview] = useState<null | number>(null)
  const [runNow, setRunNow] = useState(false)
  const { data: labels } = useLabels()
  const save = useSaveRule()
  const apply = useApplyRule()

  const chosen = () => {
    if (label) return [...actions, `label:${label}`]
    return actions
  }
  const toggle = (value: string) => {
    if (actions.includes(value)) setActions(actions.filter((a) => a !== value))
    else setActions([...actions, value])
  }
  const check = async () => {
    try {
      setPreview((await wirePreviewRule(query)).count)
    } catch (e) {
      toast.error(errorText(e))
    }
  }
  const submit = () => {
    save.mutate(
      { body: { actions: chosen(), enabled: true, name, query } },
      {
        onError: (e) => toast.error(errorText(e)),
        onSuccess: (rule) => {
          if (!runNow) {
            toast.success('Rule saved')
            onDone()
            return
          }
          apply.mutate(rule.id, {
            onError: (e) => toast.error(errorText(e)),
            onSuccess: (run) => {
              runToast(run)
              onDone()
            },
          })
        },
      }
    )
  }

  return (
    <form
      className={`${cardClass} space-y-3`}
      onSubmit={(e) => {
        e.preventDefault()
        submit()
      }}
    >
      <label className="block space-y-1 text-sm">
        <span className="text-fg-secondary">Name</span>
        <input
          className={inputClass}
          maxLength={64}
          onChange={(e) => setName(e.target.value)}
          value={name}
        />
      </label>
      <label className="block space-y-1 text-sm">
        <span className="text-fg-secondary">Mail that matches</span>
        <input
          className={`${inputClass} font-mono`}
          onChange={(e) => {
            setQuery(e.target.value)
            setPreview(null)
          }}
          placeholder="from:news@shop.example has:attachment"
          value={query}
        />
      </label>
      <fieldset className="space-y-1 text-sm">
        <legend className="text-fg-secondary mb-1">Do this</legend>
        {PLAIN_ACTIONS.map((a) => (
          <label className="mr-4 inline-flex items-center gap-1.5" key={a.value}>
            <input
              checked={actions.includes(a.value)}
              onChange={() => toggle(a.value)}
              type="checkbox"
            />
            {a.label}
          </label>
        ))}
        <label className="flex items-center gap-2 pt-1">
          Label
          <select className={inputClass} onChange={(e) => setLabel(e.target.value)} value={label}>
            <option value="">None</option>
            {(labels ?? []).map((l) => (
              <option key={l.id} value={l.id}>
                {l.name}
              </option>
            ))}
          </select>
        </label>
      </fieldset>
      <div className="flex flex-wrap items-center gap-3 text-sm">
        <button className={btnSecondary} disabled={!query.trim()} onClick={check} type="button">
          Preview
        </button>
        {preview !== null && <span>{preview} conversations match now</span>}
        {preview !== null && preview > 0 && (
          <label className="inline-flex items-center gap-1.5">
            <input checked={runNow} onChange={(e) => setRunNow(e.target.checked)} type="checkbox" />
            Also apply to them
          </label>
        )}
      </div>
      <div className="flex justify-end gap-2">
        <button className={btnSecondary} onClick={onDone} type="button">
          Cancel
        </button>
        <button
          className={btnPrimary}
          disabled={!name.trim() || !query.trim() || chosen().length === 0 || save.isPending}
          type="submit"
        >
          Save rule
        </button>
      </div>
    </form>
  )
}
