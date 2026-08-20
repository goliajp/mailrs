package jp.golia.mailrs.wire

/**
 * A meeting invitation, and the answer to it.
 *
 * Extensions, like the operator endpoints beside them, so they reach
 * the `internal` plumbing without any of it becoming public.
 */

/**
 * `GET /api/mail/messages/{uid}` — the invitation this message carries,
 * and whatever answer this reader already gave.
 *
 * Asked only when the message's own `invite_method` says there is one:
 * the event is a few kilobytes and every other message would pay for
 * it.
 */
suspend fun MailrsClient.invite(uid: Int): MailrsClient.Outcome<Wire.MessageDetail> =
    one(get("/api/mail/messages/$uid"), Wire.MessageDetail.serializer())

/**
 * `POST /api/invites/{uid}/rsvp` — answer it.
 *
 * The server records the choice **and** queues an iTIP `REPLY` to the
 * organiser. A `202` means the choice was recorded and the reply could
 * not be sent, which must not read as success: the organiser is then
 * still waiting, and a card that says "accepted" anyway is the failure
 * this whole change is about.
 */
suspend fun MailrsClient.rsvp(uid: Int, partstat: String): MailrsClient.Outcome<String> =
    post(url("/api/invites/$uid/rsvp"), """{"partstat":"$partstat"}""", authorized = true)
