import { describe, expect, it } from 'vitest'

import { joinLinkOf } from '@/lib/invite-join'

describe('the link a meeting is joined by', () => {
  // RFC 5545 has no field for it, so Teams puts it in the body and
  // names the room in LOCATION. Reading only LOCATION finds a room in
  // Santa Clara and no way to attend from Tokyo.
  it('finds a Teams link in the description', () => {
    const description =
      '________\r\nMicrosoft Teams meeting\r\nJoin on your computer:\r\n' +
      'https://teams.microsoft.com/l/meetup-join/19%3ameeting_abc/0?context=%7b%22Tid%22%3a%22x%22%7d\r\n' +
      'Meeting ID: 228 349 432 615 70'
    expect(joinLinkOf('SCL.H-120 (11) Teams Room (Santa Clara)', description)).toBe(
      'https://teams.microsoft.com/l/meetup-join/19%3ameeting_abc/0?context=%7b%22Tid%22%3a%22x%22%7d'
    )
  })

  it('finds a Zoom link in the location, which is where Zoom puts it', () => {
    expect(joinLinkOf('https://example.zoom.us/j/123456789?pwd=abc', null)).toBe(
      'https://example.zoom.us/j/123456789?pwd=abc'
    )
  })

  // A room is not a link, and offering a button that goes nowhere is
  // worse than offering none.
  it('offers nothing when there is nothing to join', () => {
    expect(joinLinkOf('Meeting room 4', 'Bring the printouts.')).toBeNull()
    expect(joinLinkOf(null, null)).toBeNull()
  })

  // Not every https:// in a mail body is a way into the meeting. An
  // unsubscribe footer or a company link would send somebody to the
  // wrong place at the moment they are trying to join.
  it('does not mistake any link for a meeting', () => {
    expect(joinLinkOf(null, 'Read the agenda at https://example.com/agenda')).toBeNull()
  })
})
