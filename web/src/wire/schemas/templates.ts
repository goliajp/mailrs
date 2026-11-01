import { z } from '@/lib/zod'

// Backend: crates/webapi/src/handlers/prefs.rs — list_templates / save_template
// (the type is `TemplateWire` in crates/core-api/src/method/admin/userdata.rs)
export const templateSchema = z.object({
  category: z.string(),
  html_body: z.string(),
  id: z.number(),
  name: z.string(),
  subject: z.string(),
  text_body: z.string(),
})

export type WireTemplate = z.infer<typeof templateSchema>

export const templatesSchema = z.array(templateSchema)

export const savedTemplateSchema = z.object({ id: z.number() })
