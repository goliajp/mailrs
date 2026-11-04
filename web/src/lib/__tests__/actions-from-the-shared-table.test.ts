// The web's action table is read from `actions.json`, which the phone apps
// ship a copy of. These check the reading: every web entry is an ActionId,
// names a word the dictionary has and an icon lucide has, and the copies
// in the two apps are the same bytes.

import type { ActionId } from '@/lib/actions'

import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

import { en } from '@/i18n/en'
import { ACTIONS, ICONS, WEB_ENTRIES } from '@/lib/actions'
import table from '@/lib/actions.json'

// Spelled out so a new ActionId without an entry, or an entry the union
// lacks, fails here rather than as an undefined at a click.
const IDS = {
  archive: true,
  'delete-forever': true,
  label: true,
  'mark-junk': true,
  'mark-not-junk': true,
  'mark-notification': true,
  'mark-promotion': true,
  'move-to-inbox': true,
  pin: true,
  read: true,
  report: true,
  snooze: true,
  star: true,
  trash: true,
  unarchive: true,
  unpin: true,
  unread: true,
  unsnooze: true,
  unstar: true,
  untrash: true,
} satisfies Record<ActionId, true>

const ROOT = join(import.meta.dirname, '..', '..', '..', '..')

describe('the shared action table', () => {
  it('gives the web exactly its ActionIds', () => {
    expect(WEB_ENTRIES.map((e) => e.id).sort()).toEqual(Object.keys(IDS).sort())
    expect(Object.keys(ACTIONS).sort()).toEqual(Object.keys(IDS).sort())
  })

  it('names words the dictionary has and icons lucide has', () => {
    for (const s of Object.values(ACTIONS)) {
      expect(s.label in en, s.label).toBe(true)
      if (s.done !== null) expect(s.done in en, s.done).toBe(true)
      expect(s.icon, s.id).toBeDefined()
    }
    for (const e of WEB_ENTRIES) expect(ICONS[e.icon.web ?? ''], e.id).toBeDefined()
  })

  it('undoes only into an action the web offers', () => {
    // the phones' label is undone by unlabel, which the web has not
    expect(table.actions.find((e) => e.id === 'label')?.undo).toBe('unlabel')
    expect(ACTIONS.label.undo).toBeNull()
    expect(ACTIONS.archive.undo).toBe('unarchive')
  })

  it('names every undo as an id the table has', () => {
    const ids = new Set(table.actions.map((e) => e.id))
    for (const e of table.actions) if (e.undo !== null) expect(ids.has(e.undo), e.id).toBe(true)
  })

  it('is the same bytes in the phone apps', () => {
    const web = readFileSync(join(ROOT, 'web', 'src', 'lib', 'actions.json'), 'utf8')
    for (const copy of [
      join('ios', 'Mailrs', 'Resources', 'actions.json'),
      join('android', 'app', 'src', 'main', 'assets', 'actions.json'),
    ]) {
      expect(readFileSync(join(ROOT, copy), 'utf8'), copy).toBe(web)
    }
  })
})
