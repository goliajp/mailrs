import { describe, expect, it } from 'vitest'

import { isOfficeFile } from '../office'

describe('isOfficeFile', () => {
  it('knows Office files by type, or by name when the type says nothing', () => {
    const docx = 'application/vnd.openxmlformats-officedocument.wordprocessingml.document'
    expect(isOfficeFile(docx, 'a.bin')).toBe(true)
    expect(isOfficeFile('application/octet-stream', 'Q3.XLSX')).toBe(true)
    expect(isOfficeFile('application/octet-stream', 'deck.pptx')).toBe(true)
    expect(isOfficeFile('application/msword', 'old.doc')).toBe(false)
    expect(isOfficeFile('application/pdf', 'x.pdf')).toBe(false)
  })
})
