# Mailrs iOS — density & throughput design

Benchmarks: Apple Mail (motion, gestures, dates), Gmail (triage speed),
and the web client's own settled decisions — which win ties, because
they are the same person's taste already paid for.

## Glass is a claim about layering

iOS 26's Liquid Glass goes on things that float *over* content — the
undo toast, the Load-images prompt — and on nothing else. Glass says
"this is above the page"; putting it on the mail itself would say the
message is hovering over something, which is untrue and unreadable.
Scroll views get the soft edge so rows dissolve into the chrome
rather than sliding under a hard line. The deployment target is
iOS 18, so every use falls back to the material it replaces rather
than to nothing.

## Tokens, not colours

The palette is the web client's, ported value for value: mailrs's web
UI runs the gds `zinc-neutral` preset, whose light and dark sets are
written out as explicit hex rather than derived, so the two clients
can hold the same colours instead of two interpretations of the same
intent. `Theme` names roles (`surface`, `fgMuted`, `danger`), never
literals, and the names match the CSS custom properties so a change
on either side is greppable from the other. It is resolved once at
the root from the effective colour scheme — no view asks which mode
it is in, because asking is how one view ends up disagreeing with
the next.

Appearance is a choice: System, Light or Dark, persisted, applied as
`preferredColorScheme` so following the system is the *absence* of an
override rather than a value that has to track it. Language and time
zone are choices too, in the same place, applied through the
environment slots SwiftUI already has for them — which is why a
chosen zone reaches the row dates and the message headers without any
screen consulting a preference.

The in-app mark is the icon's own artwork, drawn in SwiftUI from the
same geometry — not a system glyph tinted with the accent, which made
the sign-in screen blue while the home screen was red: the first two
things anyone sees, disagreeing.

Two more of the web's row decisions inherited: read rows recede to
70% (`muted` there), because unread already carries the dot and the
weight and dimming what is done is what makes a long list scannable;
and a failed send wears a danger-coloured left edge, because a status
word alone is easy to scan past.

## Identity (the visual system)

One blood-line with the web client. The accent is GOLIA blue
(#3b7ddd light / #3b82f6 dark — the gds tokens), set as the asset
catalog's AccentColor so every tint inherits it. Sender avatars are
the web's exactly: 16 tailwind-500 colors picked by the same
31-multiply hash over the address, so the same correspondent wears
the same color on every client — the hash is unit-tested against
values computed by the web's algorithm, wrapping like JS `| 0`.
The app icon is the web client's, not a second one: the red gradient,
white envelope and pink flap of `web/public/icon.svg`, redrawn
full-bleed because iOS applies its own squircle mask and the SVG's
own rounded corners would leave white showing at the mask's edge.
SF stays the typeface — on iOS the system font is the
professional choice, and identity lives in color and shape.

Rows carry the web's grammar: avatar with the unread dot on its rim
(keeping its VoiceOver label), +N extra participants, received↓
sent↑ split in a capsule chip when a thread has both directions,
an importance mark for critical/important. Thread messages are
cards on the grouped background; folded rows are quieter cards of
the same radius.

The identity reaches the edges too. The sign-in screen is a front
door: the mark, the name in rounded, one line of what this is, and a
full-width prominent button — not another table row. Sent rows wear
the recipient's avatar (a sent row's face is who it went to, mirroring
the inbox's who it came from) and their delivery states are tinted
capsules, the web's badge shape.

Message bodies follow the phone only when the mail has no opinion.
A dark thread used to show every HTML body as a white slab, which was
a deliberate choice — mail is authored against white, and handing a
dark background to a message that sets its own black text is worse
than the slab. But that reasoning only covers mail that styles
itself. `MailAppearance` draws the line: any declared colour — a
background, a text colour, a `<font>` tag — means the message is a
design and keeps its white paper; mail that declares none inherits
the app's surface, and the body becomes part of the card instead of
a bright rectangle in a dark room.

The list's title is inline, not large. A large title spends about
fifty points of every screen restating a word the toolbar has room
for, and this list is measured in rows.

## The exit is always in the same corner

A sheet with nothing to add puts Done where Settings and Drafts put
it — trailing. A sheet with an Add action keeps Done leading so the
creative action owns the confirming corner. Either way the way out is
where it was last time.

## An empty state is a conclusion

So it finishes the sentence. "No drafts" alone says the screen is
empty, which the reader can see; the line under it says where drafts
come from, which they cannot. Every absence that is not
self-evident carries one.

And a conclusion waits for its evidence. Every list is
`loading → failed → empty → content`, in that order, because "No
drafts" printed while the request is still out is a claim nobody made.
Drafts was written after the mail lists and inherited none of it: it
announced an empty list on open and then filled in underneath itself.

The failure branch needs the failure to survive the load, which is why
`(try? await client.drafts()) ?? []` had to go. Of the seven swallowed
errors in `Session`, that was the one the reader would have believed —
an empty draft list is perfectly ordinary, so the lie was plausible.
The others are honest: a missing brand icon *is* absence, and a delete
that fails leaves the row visibly there.

One swallowed error is left, knowingly: the save that runs as the
composer closes. If it fails there is no composer left to say so, and
"Cancel is not discard" quietly stops being true. Making that honest
means holding the text locally and retrying — an outbox, which is a
feature and not a polish, so it is written down here rather than
invented in passing.

## Deletion asks in proportion to what is lost

Deleting an alias asks, and an alias takes five seconds to retype.
Deleting a draft — unrecoverable prose — asked nothing, because
`onDelete` is one gesture away and nobody looked at the two side by
side. Conversations get an undo instead, which is the same protection
by a different route; drafts, inside a sheet with nowhere to put a
snackbar, get the named alert everything else uses.

## Every field is named

Including the largest one. SwiftUI's `TextEditor` has no placeholder,
so a composer whose To and Subject were labelled had an unlabelled
rectangle for the part people came to write in. The ghost sits behind
the editor and takes no touches — and the editor's own background has
to be hidden or it draws over it, which is a placeholder that is
present and invisible.

## A row that wraps is a defect

Whatever it says. A header that spills onto a second line reads as
broken before anyone finishes reading it, so every row with more than
one piece of text bounds the variable ones to a single line, and says
which yields: the name gives way before the date does, because a
truncated name is still a name and a truncated timestamp is nothing.
Addresses and filenames truncate in the middle, where the domain and
the extension survive.

State is a mark and a colour, not a sentence. "Unverified sender"
spelled out beside a name and a time was two words too many for the
line; it is an orange shield now, with the words kept as the
accessibility label — read aloud rather than competing for width.

## Typing belongs at the top

Every screen with a field puts it in the upper half. A `Form` spends
a section header, a card and two paddings on each of To and Subject,
which put the editor three hundred points down — under the keyboard,
which is where it is needed. Compose and reply use one compact line
per field and give the editor everything that is left.

## The row (the unit everything multiplies)

**Two lines, no preview.** The web made this exact call on 2026-07-17
("compact rows: no snippet/preview line") and it holds here for the
same reason: the preview line answers a question triage doesn't ask,
and costs a third of every row. The row is:

    ● sender·······················×3  14:32
      subject····························★

- Line 1: sender — semibold when unread, the weight both benchmarks
  use; message count as a quiet `×N` only when N > 1; relative date.
- Line 2: subject, secondary; star when flagged.
- Unread = bold sender + the dot. The dot keeps its accessibility
  label ("Unread") — colour alone is not a signal.
- Vertical padding 2pt, tightened insets. Target ≥ 10 rows on a 6.1"
  screen, up from 7.

**Considered and rejected: hiding the disclosure chevron.** Apple Mail
draws none and it would buy ~20pt of width, but the idiomatic hidden-
NavigationLink trick demotes the row from one labelled button to loose
static text — a real accessibility regression and a rewrite of every
row locator in the test suite, for horizontal space when the shortage
is vertical.

## Dates (`RowDate`)

Apple Mail's ladder, because it front-loads the information that
changes a decision: today → time, yesterday → "Yesterday", this week →
weekday, this year → month + day, older → date with year. "Aug 5" on
today's mail hides exactly the freshness that decides open-now versus
later. Pure classifier, unit-tested; formatting stays locale-native.

Three forms, because there are three kinds of fact, and one view
(`RowDateText`) so no surface can pick its own:

- **ladder** — a row being *scanned*. Drops whatever position in the
  list already implies.
- **stamp** — one row being *read on purpose*: an opened message, an
  audit entry. Absolute, and the year is not optional. `Aug 8, 20:17`
  is the same string for this year and for four years ago.
- **day** — a *window* rather than a moment. A DMARC report covers a
  reporting period, so a clock on it would print a precision the fact
  does not have.

The ladder was written for the conversation list and used there alone:
the thread's message rows, the sent list, the report list and the audit
log each printed their own `.month().day()`. A message from ten minutes
ago read `20:17` in the list and `8/8` in the thread it belongs to —
the same fact, two answers, and the less useful one on the screen you
opened to read it.

All three are read in the reader's calendar — the system's, with the
chosen zone and language grafted on. Those are separate environment
keys, so a row that reads one and not the other disagrees with itself.
`Calendar.reader` assembles it once and the view does the reading, so a
caller cannot forget.

## A status word that repeats the screen's title is not information

The queue row said "Waiting", which is what the word *queue* had
already said, while the wire had carried `next_retry`, `scheduled_at`
and `created_at` all along. A queue screen answers two questions — how
long has this been sitting here, when will it move — and neither was on
it.

Worse, a **scheduled** send and a **stuck** one read identically. One
is working as intended, the other needs attention, and the operator
could not tell them apart. Now the first says `Scheduled` in accent and
`Sends 20:17`; the second says `Retrying` and `Next attempt 20:17`.

A retry time in the *past* is not printed as a promise: "next attempt
20:17" shown at 21:00 accuses the queue of being broken when it is
merely busy. That rule is `QueueTiming`, pure and tested, because
"scheduled" and "overdue" differ only by the clock.

The timing is one line with the attempt count trailing it, so a narrow
phone truncates the tail instead of wrapping.

## Gestures (triage without opening)

    full swipe left   = Archive  (the benchmark triage gesture; no
                                  confirm — it is reversible, and a
                                  5-second Undo snackbar makes the
                                  reversal one tap, Gmail-style; single
                                  slot, a second archive replaces the
                                  first)
    partial left      = Archive · Delete (Delete keeps its confirm:
                                  the server unlinks files)
    full swipe right  = Read/Unread toggle (Apple Mail's)
    partial right     = Read · Star
    long press        = junk verdict, reply-level actions

Delete deliberately loses the full-swipe slot it had: the fastest
gesture in the app must not be the irreversible one.

## Batch processing (select mode)

Select → checkboxes → one action for the lot, Gmail's triage-at-scale.
The count lives in the navigation title while selecting (Apple Mail's
pattern — the bottom bar's floating pills truncate text). Read /
Archive / Delete in the bottom bar; batch delete keeps the confirm,
batch archive shares the single undo slot with the swipe and the
toast carries the count ("Archived ×N"). Sign-out is expected to move
off the toolbar when a settings surface exists; Select took the slot
priority because triage is daily and sign-out is rare.

## The thread's title block

The subject gets a line of its own above the messages — Apple Mail's
shape — because a nav bar squeezed between a back button and three
toolbar buttons showed six words and an ellipsis, and that was the
only place the subject appeared at all. Three lines at title3, then a
context line: how many messages, and who wrote them (me excluded, the
row-face rule again). The nav title is now empty; the back button
already says which list you came from.

## Remote content waits to be asked

Fetching a remote image tells the sender the message was opened, from
which address and when — a logo and a 1×1 beacon report identically.
So http and https subresources are refused until the reader taps
*Load images*, per message, never remembered: consenting to one
sender's images is not consent for the next one's. The refusal is a
content rule list, not attribute rewriting, because the pixel that
does not want to be found hides in a CSS background. Images the
message carries itself (`data:`, `cid:`) always render.

## Triage from inside the thread

Star, Archive and Delete live in the thread's bottom bar — Apple
Mail's shape — because every verdict otherwise cost a trip back to
the list and a swipe on a row you had to find again. Star stays (a
verdict that does not remove the thread should not remove the
reader from it); Archive leaves at once and lands on the list where
its undo toast is already waiting; Delete keeps its confirmation and
leaves only once the server says the files are gone.

## Serial processing (the thread view)

▲/▼ in the thread toolbar walk the list order without returning to it —
Apple Mail's chevrons. Each step is an open, so it marks read through
the same rule as any open. List order is `visibleConversations`, the
one ordering the list itself draws.

## Long threads (folding)

A thread opens with everything but the newest message folded to one
line — sender, a breath of body, a paperclip if it carries files, the
date. Both benchmarks make this call: the thread is context, the last
message is the reason you came. Tap the line to unfold; tap an open
card's header to fold it back. Expansion is derived, not latched —
the state is only the set of explicit toggles, XORed against "is the
newest" (`ThreadCollapse`), so switching threads just clears the set.

## Loading and motion (landed previously, part of this system)

Empty states wait for evidence; every row mutation animates including
rollbacks; message bodies grow and fade in once measured; send answers
through the hand. These are the "between states" half of the same
design.
