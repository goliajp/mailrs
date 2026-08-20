# mailrs-datefind

Find the dates and times somebody wrote in ordinary prose — "Friday the
21st at 2pm", "8月21日 14:00", "2026-08-21" — and hand back where they
are and what they mean.

It exists because most mail about a meeting is not an invitation. It
carries no `text/calendar` part, no `UID`, nothing to accept: just a
sentence with a time in it. Apple Mail has offered to turn those into
events since 2007, and a mail client without it makes the reader retype
what is already on the screen.

**It proposes; it never files.** A guess about a date is a guess, and
the difference between offering one and acting on one is the difference
between a useful client and an alarming one.

## What it recognises

Deliberately a small, unambiguous set, because a wrong offer is worse
than none:

| written | read as |
|---|---|
| `2026-08-21` | that date |
| `Aug 21`, `August 21st`, `21 Aug` | that date, year resolved against a reference |
| `8月21日`, `8月21日(金)` | that date |
| `14:00`, `2pm`, `2:30 PM` | that time |
| `14時`, `14時30分` | that time |

Ambiguous forms are **not** guessed at. `08/09` is the ninth of August
to half the world and the eighth of September to the other half, so it
is left alone.

## The year

A bare `Aug 21` has no year. Resolved against a reference instant —
normally the message's own `Date:` — and rolled forward when the result
is more than a month behind it: mail proposing a meeting is proposing a
future one, and "August 21" in a December message means the next one.
