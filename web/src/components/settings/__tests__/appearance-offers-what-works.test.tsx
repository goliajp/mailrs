// Appearance offered "Page size", which nothing read: lists load the next
// page as they scroll, at a size the client fixes. A control that does
// nothing is gone rather than kept.

import { cleanup, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, describe, expect, it } from 'vitest'

import { AppearanceSection } from '../appearance-section'

afterEach(cleanup)

describe('appearance settings', () => {
  it('offers no page size', () => {
    render(
      <Provider store={createStore()}>
        <AppearanceSection />
      </Provider>
    )
    expect(screen.getByText('Light')).toBeDefined()
    expect(screen.queryByLabelText('Page size')).toBeNull()
  })
})
