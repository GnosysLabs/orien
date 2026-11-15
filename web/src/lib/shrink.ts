// Pictures are shrunk in the browser before they are sent: phones and
// cameras make pictures of 10 to 40 MB, far more than the marketplace keeps
// (it crops every picture to at most 1500 pixels wide) and more than it
// accepts (8 MB).

/** The longest side kept, in pixels: well above anything shown. */
const LONGEST_SIDE = 2560
/** Pictures this small and no larger than LONGEST_SIDE are sent as they are. */
const SMALL_ENOUGH = 2 << 20
/** What the marketplace accepts, with room to spare. */
const MOST = 7.5 * (1 << 20)

/** The picture, small enough to send: as it is if it already is, otherwise
 * redrawn as a JPEG of at most LONGEST_SIDE pixels on its longer side. A
 * picture the browser cannot read goes as it is, for the server to judge. */
export async function shrinkPicture(file: File): Promise<Blob> {
  let bitmap: ImageBitmap
  try {
    bitmap = await createImageBitmap(file, { imageOrientation: 'from-image' })
  } catch {
    return file
  }
  try {
    const scale = Math.min(1, LONGEST_SIDE / Math.max(bitmap.width, bitmap.height))
    if (scale === 1 && file.size <= SMALL_ENOUGH) return file
    const canvas = document.createElement('canvas')
    canvas.width = Math.round(bitmap.width * scale)
    canvas.height = Math.round(bitmap.height * scale)
    const context = canvas.getContext('2d')
    if (!context) return file
    context.drawImage(bitmap, 0, 0, canvas.width, canvas.height)
    for (const quality of [0.9, 0.8, 0.65]) {
      const blob = await new Promise<Blob | null>((done) => canvas.toBlob(done, 'image/jpeg', quality))
      if (blob && blob.size <= MOST) return blob
    }
    return file
  } finally {
    bitmap.close()
  }
}
