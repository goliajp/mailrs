import { cleanup, render, screen, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it } from 'vitest'

const { Settings } = await import('@/pages/settings')

afterEach(cleanup)

function tabsOf(group: string): string[] {
  const list = screen.getByRole('tablist', { name: group })
  return within(list)
    .getAllByRole('tab')
    .map((tab) => tab.textContent ?? '')
}

describe('the settings section groups', () => {
  it('files Notifications under the group named after it', () => {
    render(
      <MemoryRouter>
        <Settings />
      </MemoryRouter>
    )
    expect(tabsOf('Appearance & notifications')).toEqual(['Appearance', 'Notifications'])
    expect(tabsOf('Mail handling')).not.toContain('Notifications')
  })
})
