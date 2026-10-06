import { finishIcon, GLYPH_INSET, iconShape } from '@/lib/icon-finish'

// a blob: url is same-origin, so the canvas is not tainted and the
// pixels can be read back
export async function finishedIconUrl(raw: string): Promise<string> {
  const img = new Image()
  img.src = raw
  await img.decode()
  const side = Math.min(128, Math.max(64, Math.min(img.naturalWidth, img.naturalHeight)))
  const canvas = document.createElement('canvas')
  canvas.width = side
  canvas.height = side
  const ctx = canvas.getContext('2d', { willReadFrequently: true })!
  drawCover(ctx, img, 0, side)
  let pixels = ctx.getImageData(0, 0, side, side)
  const shape = iconShape(pixels.data, side)
  if (shape.kind === 'glyph') {
    ctx.clearRect(0, 0, side, side)
    const inset = Math.round(side * GLYPH_INSET)
    drawCover(ctx, img, inset, side - 2 * inset)
    pixels = ctx.getImageData(0, 0, side, side)
  }
  finishIcon(pixels.data, side, shape)
  ctx.putImageData(pixels, 0, 0)
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'))
  URL.revokeObjectURL(raw)
  return URL.createObjectURL(blob!)
}

// object-cover into the square at (at, at) of size `box`: the centre
// square of the image, so a wide wordmark is cropped rather than
// squeezed. the crop is done with the destination rectangle and a clip,
// never a source rectangle: safari draws nothing for an svg given one
function drawCover(ctx: CanvasRenderingContext2D, img: HTMLImageElement, at: number, box: number) {
  const scale = box / Math.min(img.naturalWidth, img.naturalHeight)
  const w = img.naturalWidth * scale
  const h = img.naturalHeight * scale
  ctx.save()
  ctx.beginPath()
  ctx.rect(at, at, box, box)
  ctx.clip()
  ctx.imageSmoothingQuality = 'high'
  ctx.drawImage(img, at + (box - w) / 2, at + (box - h) / 2, w, h)
  ctx.restore()
}
