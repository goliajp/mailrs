import { z } from '@/lib/zod'

// Backend: crates/webapi/src/handlers/storage.rs — fn get_storage
// `quota_bytes` is null when the account has no quota
export const storageSchema = z.object({
  quota_bytes: z.number().nullable(),
  used_bytes: z.number(),
})

export type WireStorage = z.infer<typeof storageSchema>
