# Mailrs iOS — density & throughput design

Benchmarks: Apple Mail (motion, gestures, dates), Gmail (triage speed),
and the web client's own settled decisions — which win ties, because
they are the same person's taste already paid for.

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
                                  confirm — it is reversible)
    partial left      = Archive · Delete (Delete keeps its confirm:
                                  the server unlinks files)
    full swipe right  = Read/Unread toggle (Apple Mail's)
    partial right     = Read · Star
    long press        = junk verdict, reply-level actions

Delete deliberately loses the full-swipe slot it had: the fastest
gesture in the app must not be the irreversible one.

## Serial processing (the thread view)

▲/▼ in the thread toolbar walk the list order without returning to it —
Apple Mail's chevrons. Each step is an open, so it marks read through
the same rule as any open. List order is `visibleConversations`, the
one ordering the list itself draws.

## Loading and motion (landed previously, part of this system)

Empty states wait for evidence; every row mutation animates including
rollbacks; message bodies grow and fade in once measured; send answers
through the hand. These are the "between states" half of the same
design.
