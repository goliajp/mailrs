import { afterEach, describe, expect, it } from 'vitest'

import { hasUnsentText } from '@/hooks/use-reload-on-new-build'

afterEach(() => {
  document.body.innerHTML = ''
})

describe('a page with something being written is not reloaded', () => {
  it('sees text typed into a text area', () => {
    document.body.innerHTML = '<textarea></textarea>'
    expect(hasUnsentText(document)).toBe(false)
    document.querySelector('textarea')!.value = 'half a reply'
    expect(hasUnsentText(document)).toBe(true)
  })

  it('sees text in an editable region', () => {
    document.body.innerHTML = '<div contenteditable="true">  </div>'
    expect(hasUnsentText(document)).toBe(false)
    document.body.innerHTML = '<div contenteditable="true">Dear Bob</div>'
    expect(hasUnsentText(document)).toBe(true)
  })
})
