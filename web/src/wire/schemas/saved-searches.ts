import { z } from '@/lib/zod'

// Backend: crates/webapi/src/handlers/saved_searches.rs — fn list / fn create
// (the type is crates/core-sidestate/src/families/saved_searches.rs `SavedSearch`)
export const savedSearchSchema = z.object({
  created_at: z.number(),
  id: z.string(),
  name: z.string(),
  query: z.string(),
})

export type WireSavedSearch = z.infer<typeof savedSearchSchema>

export const savedSearchesSchema = z.array(savedSearchSchema)
