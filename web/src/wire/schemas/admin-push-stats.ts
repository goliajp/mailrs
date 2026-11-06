/**
 * Pushes per channel per day, summed over every account.
 *
 * Backend: crates/webapi/src/handlers/admin_push_stats.rs — fn push_stats
 * Every channel on every day carries all four counts; zero is a real zero.
 */

import { z } from '@/lib/zod'

const channelTotalsSchema = z.object({
  failed: z.number(),
  sent: z.number(),
  suppressed_policy: z.number(),
  suppressed_quiet: z.number(),
})

export const adminPushStatsSchema = z.object({
  accounts: z.number(),
  days: z.array(
    z.object({
      apns: channelTotalsSchema,
      day: z.string(),
      unifiedpush: channelTotalsSchema,
      web: channelTotalsSchema,
    })
  ),
})

export type AdminPushStats = z.infer<typeof adminPushStatsSchema>
export type ChannelTotals = z.infer<typeof channelTotalsSchema>
