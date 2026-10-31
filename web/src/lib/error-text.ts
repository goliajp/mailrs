/** The thrown error's own message when there is one, else the caller's. */
export function errorText(error: unknown, fallback: string): string {
  if (error instanceof Error) return error.message
  return fallback
}
