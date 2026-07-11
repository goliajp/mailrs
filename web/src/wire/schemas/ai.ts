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
