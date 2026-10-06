export type Rgb = [number, number, number]

// a black disc on the dark theme's page (#18181b) has no visible edge, so
// near-black is lifted to this grey; a transparent light glyph sits on it too
export const LIFTED_DARK: Rgb = [63, 63, 70]
const WHITE: Rgb = [255, 255, 255]

const LIGHT_GLYPH = 0.4
const NEAR_BLACK = 0.03
const OPAQUE = 200

// a sender icon is drawn as a disc (object-cover, rounded-full) and is
// made fully opaque here, in place, so it never shows the page through:
// - an icon with its own background (a rounded-square logo) has the
//   transparency outside that background filled with its colour, so the
//   corners do not show a foreign rim
// - any transparency left, a bare glyph's or the inside of a ring logo,
//   gets a plate: white unless the glyph itself is light
// - a near-black background is lifted to grey, white staying white, so a
//   black disc does not merge with the dark page
export function finishIcon(rgba: Uint8ClampedArray, side: number): void {
  const rim = rimColour(rgba, side)
  let plate = WHITE
  if (glyphLuminance(rgba) > LIGHT_GLYPH) plate = LIFTED_DARK
  if (rim) fillOutside(rgba, side, rim)
  composite(rgba, plate)
  if (rim && luminance(rim) < NEAR_BLACK) lift(rgba, rim, LIFTED_DARK)
}

function composite(rgba: Uint8ClampedArray, under: Rgb): void {
  for (let i = 0; i < rgba.length; i += 4) {
    const a = rgba[i + 3] / 255
    for (let c = 0; c < 3; c++) rgba[i + c] = rgba[i + c] * a + under[c] * (1 - a)
    rgba[i + 3] = 255
  }
}

// flood fill from the image border through non-opaque pixels
function fillOutside(rgba: Uint8ClampedArray, side: number, colour: Rgb): void {
  const seen = new Uint8Array(side * side)
  const stack: number[] = []
  for (let k = 0; k < side; k++) {
    stack.push(k, (side - 1) * side + k, k * side, k * side + side - 1)
  }
  while (stack.length > 0) {
    const p = stack.pop()!
    if (seen[p] || rgba[p * 4 + 3] >= OPAQUE) continue
    seen[p] = 1
    const a = rgba[p * 4 + 3] / 255
    for (let c = 0; c < 3; c++) rgba[p * 4 + c] = rgba[p * 4 + c] * a + colour[c] * (1 - a)
    rgba[p * 4 + 3] = 255
    const x = p % side
    if (x > 0) stack.push(p - 1)
    if (x < side - 1) stack.push(p + 1)
    if (p >= side) stack.push(p - side)
    if (p < side * (side - 1)) stack.push(p + side)
  }
}

function glyphLuminance(rgba: Uint8ClampedArray): number {
  let weight = 0
  let sum = 0
  for (let i = 0; i < rgba.length; i += 4) {
    const a = rgba[i + 3] / 255
    if (a === 0) continue
    sum += luminance([rgba[i], rgba[i + 1], rgba[i + 2]]) * a
    weight += a
  }
  if (weight === 0) return 0
  return sum / weight
}

// a levels adjustment: `from` maps to `to`, white stays white, so the
// glyph and its antialiased edge keep their shape
function lift(rgba: Uint8ClampedArray, from: Rgb, to: Rgb): void {
  for (let i = 0; i < rgba.length; i += 4) {
    for (let c = 0; c < 3; c++) {
      const span = 255 - from[c]
      if (span <= 0) continue
      rgba[i + c] = to[c] + ((rgba[i + c] - from[c]) * (255 - to[c])) / span
    }
  }
}

function linear(channel: number): number {
  const c = channel / 255
  if (c <= 0.04045) return c / 12.92
  return ((c + 0.055) / 1.055) ** 2.4
}

function luminance([r, g, b]: Rgb): number {
  return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

// the icon's own background: walking in from the disc's edge along each
// of RAYS rays, the first opaque pixel is the outer layer. it counts as a
// background when nearly every ray meets it in the outer part of the disc
// (an enclosing shape such as a disc or a padded rounded square; the gaps
// between a clover's leaves let rays through), most rays agree on its
// colour, and what it encloses is mostly opaque (a ring, a q or a dotted
// globe encloses empty space and stays a glyph). null for a glyph on
// transparency
const RAYS = 64
const REACH = 0.35

function distance(a: Rgb, b: Rgb): number {
  return Math.abs(a[0] - b[0]) + Math.abs(a[1] - b[1]) + Math.abs(a[2] - b[2])
}

function rimColour(rgba: Uint8ClampedArray, side: number): null | Rgb {
  const centre = (side - 1) / 2
  const hits: Rgb[] = []
  for (let k = 0; k < RAYS; k++) {
    const angle = (2 * Math.PI * k) / RAYS
    for (let r = side / 2 - 0.5; r >= side * REACH; r -= 0.5) {
      const x = Math.round(centre + r * Math.cos(angle))
      const y = Math.round(centre + r * Math.sin(angle))
      const i = (y * side + x) * 4
      if (rgba[i + 3] < OPAQUE) continue
      hits.push([rgba[i], rgba[i + 1], rgba[i + 2]])
      break
    }
  }
  if (hits.length < RAYS * 0.95) return null
  const median = [0, 1, 2].map((c) => {
    const sorted = hits.map((p) => p[c]).sort((a, b) => a - b)
    return sorted[sorted.length >> 1]
  }) as Rgb
  const agree = hits.filter((p) => distance(p, median) < 60).length
  if (agree < RAYS * 0.8) return null
  let inner = 0
  let filled = 0
  for (let y = 0; y < side; y++) {
    for (let x = 0; x < side; x++) {
      if (Math.hypot(x - centre, y - centre) >= side * REACH) continue
      inner++
      if (rgba[(y * side + x) * 4 + 3] >= OPAQUE) filled++
    }
  }
  if (filled < inner * 0.6) return null
  return median
}
