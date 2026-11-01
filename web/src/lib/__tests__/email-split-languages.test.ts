// The composer writes its quote line in the reader's language, so the
// quote it introduces folds in every one of them.

import { describe, expect, it } from 'vitest'

import { splitTextEmail } from '@/lib/email-split'

describe('quote lines in each language the composer writes', () => {
  for (const line of [
    'On 2026/11/01 10:00, Ann <ann@example.com> wrote:',
    '2026/11/01 10:00，Ann <ann@example.com> 写道：',
    '2026/11/01 10:00、Ann <ann@example.com> さんは書きました：',
  ]) {
    it(line, () => {
      const parts = splitTextEmail(`Thanks, see below.\n\n${line}\n> the original`)
      expect(parts.body.trim()).toBe('Thanks, see below.')
      expect(parts.quoted).toContain('> the original')
    })
  }
})
