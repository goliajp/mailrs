// What kind of invitation this is, and whether it wants an answer.
// Rules, not components — the file they came from cannot export them.

/// Whether to offer Yes / Maybe / No.
///
/// Only a `REQUEST` asks the reader anything. A `PUBLISH` is a
/// notice — a newsletter's event feed — and a `REPLY` is somebody
/// else's answer arriving; offering to accept either sends an iTIP
/// message to a party who did not ask for one, which organiser clients
/// treat as anything from noise to an error.
export function answerWanted(method: string): boolean {
  return method.toUpperCase() === 'REQUEST'
}

/// Whether to offer "add to my calendar" instead.
export function fileableWithoutAnswer(method: string): boolean {
  const m = method.toUpperCase()
  return m === 'PUBLISH' || m === 'PUBLISHED'
}

/// The label above the card.
///
/// `METHOD:UPDATE` exists in RFC 5546 and almost nobody sends it:
/// Exchange re-sends the whole invitation as a `REQUEST` with a higher
/// `SEQUENCE`, which is how a Teams meeting that has been moved three
/// times arrives. Calling that "New invite" — which this did until
/// 2026-08-20 — tells the reader the opposite of what happened.
export function inviteBadge(
  method: string,
  sequence: number
): { className: string; label: string } {
  switch (method.toUpperCase()) {
    case 'CANCEL':
      return { className: 'bg-red-500/15 text-red-300', label: 'Cancelled' }
    case 'COUNTER':
      return { className: 'bg-amber-500/15 text-amber-300', label: 'Counter-proposed' }
    case 'PUBLISH':
      return { className: 'bg-zinc-500/15 text-zinc-300', label: 'Event' }
    case 'REPLY':
      return { className: 'bg-blue-500/15 text-blue-300', label: 'Reply' }
    case 'REQUEST':
      return sequence > 0
        ? { className: 'bg-sky-500/15 text-sky-300', label: 'Updated invite' }
        : { className: 'bg-emerald-500/15 text-emerald-300', label: 'New invite' }
    case 'UPDATE':
      return { className: 'bg-sky-500/15 text-sky-300', label: 'Updated invite' }
    default:
      return { className: 'bg-zinc-500/15 text-zinc-300', label: method }
  }
}
