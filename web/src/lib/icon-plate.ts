export type IconPlate = 'black' | 'white'

// a sender icon is often a transparent favicon; drawn straight onto the
// page a dark logo vanishes on a dark theme and a white one on a light
// theme. the plate is chosen from the icon itself, never the theme, from
// the alpha-weighted mean luminance of its visible pixels
const LIGHT_LOGO = 0.4

export function iconPlate(rgba: Uint8ClampedArray): IconPlate {
  let weight = 0
  let sum = 0
  for (let i = 0; i < rgba.length; i += 4) {
    const a = rgba[i + 3] / 255
    if (a === 0) continue
    const l = 0.2126 * linear(rgba[i]) + 0.7152 * linear(rgba[i + 1]) + 0.0722 * linear(rgba[i + 2])
    sum += l * a
    weight += a
  }
  if (weight === 0) return 'white'
  const lum = sum / weight
  // black only for clearly light logos. the wcag crossover (≈ 0.18) sends
  // mid-tone blues and greens to black, and a black plate disappears into
  // a dark page, leaving the logo as hard to see as with no plate; a
  // mid-tone logo on white reads on both themes
  if (lum > LIGHT_LOGO) return 'black'
  return 'white'
}

function linear(channel: number): number {
  const c = channel / 255
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
}
