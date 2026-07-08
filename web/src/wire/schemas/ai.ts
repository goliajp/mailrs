/**
 * AI feature wire schemas — v2.1 §10.1 (2026-07-08).
 */

import { z } from 'zod'

export const polishResultSchema = z.object({
  message: z.string().optional(),
  polished: z.string().optional(),
  success: z.boolean(),
})

export type WirePolishResult = z.infer<typeof polishResultSchema>

export const replySuggestResultSchema = z.object({
  message: z.string().optional(),
  success: z.boolean(),
  suggestions: z.array(z.string()).default([]),
})

export type WireReplySuggestResult = z.infer<typeof replySuggestResultSchema>

export const generateSubjectResultSchema = z.object({
  message: z.string().optional(),
  subject: z.string().optional(),
  success: z.boolean(),
})

export type WireGenerateSubjectResult = z.infer<typeof generateSubjectResultSchema>

/**
 * `/mail/render-preview` — backend actually returns
 * `{png_base64, fallback_html?}` (see
 * `crates/webapi/src/handlers/misc.rs::render_preview` line 190).
 * Frontend `RenderResult` in `render-preview.tsx` fabricates a
 * different `{previews[], errors[], error?}` shape that never
 * matched — the panel was silently broken. Schema is permissive
 * for now (accepts either shape) so the caller can pattern-match;
 * full UI reconciliation is a follow-up.
 */
export const renderResultSchema = z
  .object({
    error: z.string().optional(),
    errors: z.array(z.string()).optional(),
    fallback_html: z.string().optional(),
    png_base64: z.string().nullable().optional(),
    previews: z
      .array(
        z.object({
          height: z.number().optional(),
          image_url: z.string().optional(),
          name: z.string(),
          width: z.number().optional(),
        })
      )
      .optional(),
  })
  .passthrough()

export type WireRenderResult = z.infer<typeof renderResultSchema>
