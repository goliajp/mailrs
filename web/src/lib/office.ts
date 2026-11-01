/**
 * Whether an attachment is a Word, Excel or PowerPoint file, by its type or,
 * for a generic one, by its name — the same rule the server reads them by
 * (`attachment_extract::office::office_kind`).
 */
export function isOfficeFile(contentType: string, filename: string): boolean {
  const ct = contentType.toLowerCase()
  if (
    ct.includes('wordprocessingml.document') ||
    ct.includes('spreadsheetml.sheet') ||
    ct.includes('presentationml.presentation')
  ) {
    return true
  }
  return /\.(docx|xlsx|pptx)$/i.test(filename)
}
