import type { Key } from '@/i18n'

import { describe, expect, it } from 'vitest'

import { LANGS, translate } from '@/i18n'
import table from '@/lib/wording.json'

// The phones' half is scripts/check-wording.sh and each app's own test;
// this is the web's: what `t` returns is what the shared table says.
describe('the wording table', () => {
  it('is what the web shows, in every language', () => {
    for (const lang of LANGS) {
      for (const term of table.terms) {
        for (const key of term.web) {
          expect([term.id, lang, translate(lang, key as Key)]).toEqual([
            term.id,
            lang,
            term.text[lang],
          ])
        }
      }
    }
  })

  it('fills in a placeholder the table writes', () => {
    expect(translate('ja', 'invite.organizer', { name: 'Chris' })).toBe('主催者：Chris')
  })
})
