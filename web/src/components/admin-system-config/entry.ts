// Backend: crates/webapi/src/handlers/system_config.rs — fn get_system_config
export type ConfigEntry = {
  default_value: null | string
  description: string
  /** false for settings read once at process start, and for keys nothing reads */
  editable: boolean
  /** where a read-only setting is set */
  env_var?: string
  group: string
  key: string
  note?: null | string
  source: string
  /** unix seconds; only on a value saved from this page or the MCP tool */
  updated_at?: number
  updated_by?: string
  /** null when webapi cannot see the value */
  value: null | string
}

/** "Reset to default" for a setting with a default; "Remove" for a key nothing reads. */
export function resetVerb(entry: ConfigEntry): string {
  if (entry.editable) return 'Reset to default'
  return 'Remove'
}
