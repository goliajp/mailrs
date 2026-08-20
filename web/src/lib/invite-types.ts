// The shapes the invitation endpoints answer with, shared by the card
// and by the rules beside it.
//
// Backend: `crates/webapi/src/handlers/complete.rs` —
// `get_message_single`'s `invite_payload`, which is
// `mailrs_ical::ParsedInvite` plus the two resolved instants.

export type Attendee = {
  cn: null | string
  email: string
  partstat: string
  role: string
  rsvp: boolean
}
