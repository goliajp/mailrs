import { finishIcon, GLYPH_INSET, iconShape } from '@/lib/icon-finish'

// a blob: url is same-origin, so the canvas is not tainted and the
// pixels can be read back
export async function finishedIconUrl(raw: string): Promise<string> {
  const img = new Image()
  img.src = raw
  await img.decode()
  const side = Math.min(128, Math.max(64, Math.max(img.naturalWidth, img.naturalHeight)))
  const canvas = document.createElement('canvas')
  canvas.width = side
  canvas.height = side
  const ctx = canvas.getContext('2d', { willReadFrequently: true })!
  drawContain(ctx, img, 0, side)
  let pixels = ctx.getImageData(0, 0, side, side)
  const shape = iconShape(pixels.data, side)
  if (shape.kind === 'glyph') {
    ctx.clearRect(0, 0, side, side)
    const inset = Math.round(side * GLYPH_INSET)
    drawContain(ctx, img, inset, side - 2 * inset)
    pixels = ctx.getImageData(0, 0, side, side)
  }
  finishIcon(pixels.data, side, shape)
  ctx.putImageData(pixels, 0, 0)
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'))
  URL.revokeObjectURL(raw)
  return URL.createObjectURL(blob!)
}

// the whole image, centred in the square at (at, at) of size `box`. a wide
// logo is not cropped to its centre square: cloudflare's bimi svg is 2.2:1
// and its centre is solid cloud, so a crop drew a plain orange disc. the
// destination rectangle is never a source rectangle: safari draws nothing
// for an svg given one
function drawContain(
  ctx: CanvasRenderingContext2D,
  img: HTMLImageElement,
  at: number,
  box: number
) {
  const scale = box / Math.max(img.naturalWidth, img.naturalHeight)
  const w = img.naturalWidth * scale
  const h = img.naturalHeight * scale
  ctx.imageSmoothingQuality = 'high'
  ctx.drawImage(img, at + (box - w) / 2, at + (box - h) / 2, w, h)
}
