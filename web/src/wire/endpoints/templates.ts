/**
 * The signed-in user's message templates (`crates/webapi/src/handlers/prefs.rs`).
 * Saving always creates a new template; there is no update.
 */

import { wireFetch } from '../client'
import { emptyResponseSchema } from '../schemas/mutations'
import { savedTemplateSchema, templatesSchema, type WireTemplate } from '../schemas/templates'

export async function wireCreateTemplate(t: {
  name: string
  subject: string
  text_body: string
}): Promise<number> {
  const out = await wireFetch(savedTemplateSchema, {
    body: { ...t, category: '', html_body: '', is_default: false },
    method: 'POST',
    path: '/mail/templates',
  })
  return out.id
}

export async function wireDeleteTemplate(id: number): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: `/mail/templates/${id}`,
  })
}

export function wireListTemplates(signal?: AbortSignal): Promise<WireTemplate[]> {
  return wireFetch(templatesSchema, { path: '/mail/templates', signal })
}
