import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { useT } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { queryClient } from '@/lib/query-client'
import { mailKeys } from '@/lib/query-keys'
import {
  wireCreateTemplate,
  wireDeleteTemplate,
  wireListTemplates,
} from '@/wire/endpoints/templates'

import { btnPrimary, cardClass, ConfirmDialog, inputClass, SectionHeader } from './_shared'

/** Templates the composer offers in its "Template" picker; the same list, so a change shows there at once. */
export function TemplatesSection() {
  const t = useT()
  const { data: templates = [], isLoading } = useQuery({
    queryKey: mailKeys.templates(),
    queryFn: ({ signal }) => wireListTemplates(signal),
  })
  const [name, setName] = useState('')
  const [subject, setSubject] = useState('')
  const [body, setBody] = useState('')
  const [saving, setSaving] = useState(false)
  const [deleting, setDeleting] = useState<null | number>(null)

  const refresh = () => queryClient.invalidateQueries({ queryKey: mailKeys.templates() })

  const create = async () => {
    setSaving(true)
    try {
      await wireCreateTemplate({ name: name.trim(), subject, text_body: body })
      setName('')
      setSubject('')
      setBody('')
      toast.success(t('settings.templates.saved'))
      void refresh()
    } catch (e) {
      toast.error(errorText(e, t('settings.templates.saveFailed')))
    } finally {
      setSaving(false)
    }
  }

  const remove = async (id: number) => {
    setDeleting(null)
    try {
      await wireDeleteTemplate(id)
      void refresh()
    } catch (e) {
      toast.error(errorText(e, t('settings.templates.deleteFailed')))
    }
  }

  return (
    <div className="space-y-6">
      <SectionHeader title={t('settings.cat.templates')} />
      <p className="text-fg-secondary text-sm">{t('settings.templates.intro')}</p>

      <div className={cardClass + ' space-y-3'}>
        <input
          aria-label={t('settings.templates.name')}
          className={inputClass}
          maxLength={100}
          onChange={(e) => setName(e.target.value)}
          placeholder={t('settings.templates.name')}
          value={name}
        />
        <input
          aria-label={t('settings.templates.subject')}
          className={inputClass}
          onChange={(e) => setSubject(e.target.value)}
          placeholder={t('settings.templates.subject')}
          value={subject}
        />
        <textarea
          aria-label={t('settings.templates.body')}
          className={inputClass + ' min-h-32 font-mono'}
          onChange={(e) => setBody(e.target.value)}
          placeholder={t('settings.templates.bodyPlaceholder')}
          value={body}
        />
        <button
          className={btnPrimary}
          disabled={saving || name.trim() === '' || body.trim() === ''}
          onClick={create}
          type="button"
        >
          {t('settings.templates.add')}
        </button>
      </div>

      {isLoading && <p className="text-fg-muted text-sm">{t('common.loading')}</p>}
      {!isLoading && templates.length === 0 && (
        <p className="text-fg-muted text-sm">{t('settings.templates.empty')}</p>
      )}
      <ul className="space-y-2">
        {templates.map((tpl) => (
          <li className={cardClass + ' flex items-start justify-between gap-4'} key={tpl.id}>
            <div className="min-w-0">
              <p className="truncate text-sm font-medium">{tpl.name}</p>
              {tpl.subject && <p className="text-fg-secondary truncate text-xs">{tpl.subject}</p>}
              <p className="text-fg-muted mt-1 line-clamp-2 text-xs whitespace-pre-wrap">
                {tpl.text_body}
              </p>
            </div>
            <button
              aria-label={t('settings.templates.deleteNamed', { name: tpl.name })}
              className="text-fg-muted hover:text-danger shrink-0 text-xs whitespace-nowrap"
              onClick={() => setDeleting(tpl.id)}
              type="button"
            >
              {t('labels.delete')}
            </button>
          </li>
        ))}
      </ul>
      {deleting !== null && (
        <ConfirmDialog
          message={t('settings.templates.deleteConfirm')}
          onCancel={() => setDeleting(null)}
          onConfirm={() => void remove(deleting)}
        />
      )}
    </div>
  )
}
