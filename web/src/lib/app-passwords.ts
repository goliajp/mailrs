export const appPasswordsKey = ['settings', 'app-passwords'] as const

/** A password as it is shown: four groups of four letters. */
export function grouped(password: string): string {
  return password.match(/.{1,4}/g)?.join(' ') ?? password
}
