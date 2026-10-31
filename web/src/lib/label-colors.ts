/** A label colour as a CSS colour; the names are the server's. */
const COLOR: Record<string, string> = {
  blue: '#3b7ddd',
  gray: '#71717a',
  green: '#0ca678',
  orange: '#e67700',
  purple: '#7950f2',
  red: '#e03131',
  teal: '#0c8599',
  yellow: '#d9a400',
}

export function labelColor(color: string): string {
  return COLOR[color] ?? COLOR.gray
}
