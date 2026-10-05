export type IconPlate = 'black' | 'white'

// a sender icon is often a transparent favicon; drawn straight onto the
// page a dark logo vanishes on a dark theme and a white one on a light
// theme. the plate is chosen from the icon itself, never the theme:
// whichever of black or white contrasts more with the alpha-weighted
// mean luminance of its visible pixels
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
  // wcag contrast against white is 1.05 / (lum + 0.05), against black
  // (lum + 0.05) / 0.05; they are equal at lum ≈ 0.179
  if (1.05 / (lum + 0.05) >= (lum + 0.05) / 0.05) return 'white'
  return 'black'
}

function linear(channel: number): number {
  const c = channel / 255
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
}
