# Mailrs iOS — density & throughput design

Benchmarks: Apple Mail (motion, gestures, dates), Gmail (triage speed),
and the web client's own settled decisions — which win ties, because
they are the same person's taste already paid for.

## Identity (the visual system)

One blood-line with the web client. The accent is GOLIA blue
(#3b7ddd light / #3b82f6 dark — the gds tokens), set as the asset
catalog's AccentColor so every tint inherits it. Sender avatars are
the web's exactly: 16 tailwind-500 colors picked by the same
31-multiply hash over the address, so the same correspondent wears
the same color on every client — the hash is unit-tested against
values computed by the web's algorithm, wrapping like JS `| 0`.
The app icon is drawn geometry (PIL, checked in as 1024px): the
accent field, a white envelope, signal rings radiating from the
flap. SF stays the typeface — on iOS the system font is the
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
