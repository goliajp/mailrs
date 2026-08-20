// Where a meeting is actually joined. Split out of invite-card because
// it is a rule, not a component — and because the lint that forbids a
// component file exporting plain functions is right about why.

/// The link to actually join, out of wherever the producer put it.
///
/// Teams and Zoom both put it in the location or the description rather
/// than in a field of its own — there is no such field in RFC 5545 — so
/// this reads the two places they use and takes the first URL that
/// belongs to a conferencing host.
export function joinLinkOf(
  location: null | string | undefined,
  description: null | string | undefined
): null | string {
  const hosts =
    /^(https:\/\/[^\s<>"]*\b(teams\.microsoft\.com|zoom\.us|meet\.google\.com|webex\.com|whereby\.com)[^\s<>"]*)/i
  for (const field of [location, description]) {
    if (!field) continue
    for (const token of field.split(/[\s<>"]+/)) {
      const m = hosts.exec(token)
      if (m) return m[1]
    }
  }
  return null
}
