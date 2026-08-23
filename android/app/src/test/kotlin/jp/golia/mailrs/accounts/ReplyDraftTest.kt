package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** What a reply starts out as. */
class ReplyDraftTest {
    private fun headers(
        from: String = "Ada <ada@example.com>",
        replyTo: String = "",
        subject: String = "Lunch",
        id: String = "<m1@example.com>",
        references: List<String> = emptyList(),
    ) = MessageHeaders.Parsed(
        messageId = id, from = from, subject = subject,
        replyTo = replyTo, references = references,
    )

    private val me = MailAccount.make("me@example.com", "Me", 0)

    /**
     * That is the entire purpose of the header, and ignoring it sends
     * replies to a no-reply address.
     */
    @Test
    fun `reply to wins over from`() {
        assertEquals("list@example.com", ReplyDraft.recipient(headers(replyTo = "list@example.com")))
        assertEquals("Ada <ada@example.com>", ReplyDraft.recipient(headers()))
        // Whitespace is not an address.
        assertEquals("Ada <ada@example.com>", ReplyDraft.recipient(headers(replyTo = "   ")))
    }

    /**
     * One `Re:`, never two — a conversation that has been round a few
     * times otherwise reads `Re: Re: Re:`, and some clients thread on the
     * subject.
     */
    @Test
    fun `the subject gains one re and only one`() {
        assertEquals("Re: Lunch", ReplyDraft.subject("Lunch"))
        assertEquals("Re: Lunch", ReplyDraft.subject("Re: Lunch"))
        assertEquals("Re: Lunch", ReplyDraft.subject("RE: Re: Lunch"))
        assertEquals("Re: Lunch", ReplyDraft.subject("re : Lunch"))
    }

    /** The prefixes a phone in Japan or China actually sends. */
    @Test
    fun `localised prefixes count too`() {
        assertEquals("Re: 午饭", ReplyDraft.subject("回复: 午饭"))
        assertEquals("Re: 午饭", ReplyDraft.subject("答复: Re: 午饭"))
    }

    /** A subject that is only a prefix, and one that is nothing. */
    @Test
    fun `an empty subject still becomes a reply`() {
        assertEquals("Re:", ReplyDraft.subject(""))
        assertEquals("Re: ", ReplyDraft.subject("Re:"))
    }

    /**
     * Threading is carried, or the reply starts a new conversation in
     * every client that reads it.
     */
    @Test
    fun `the conversation is carried`() {
        val draft = ReplyDraft.make(headers(references = listOf("<m0@example.com>")), me)
        assertEquals("<m1@example.com>", draft.inReplyTo)
        assertEquals(listOf("<m0@example.com>", "<m1@example.com>"), draft.references)
    }

    /** A message already in its own References must not be listed twice. */
    @Test
    fun `the parent is not repeated`() {
        val draft = ReplyDraft.make(
            headers(references = listOf("<m0@example.com>", "<m1@example.com>")), me,
        )
        assertEquals(listOf("<m0@example.com>", "<m1@example.com>"), draft.references)
    }

    /**
     * Quoting, with an attribution line — and nothing at all when there
     * is nothing to quote, because an attribution above an empty quote
     * reads as a message that failed to load.
     */
    @Test
    fun `the original is quoted`() {
        val out = ReplyDraft.quoted("one\n\ntwo", headers())
        assertTrue(out, out.contains("Ada <ada@example.com> wrote:"))
        assertTrue(out, out.contains("> one"))
        assertTrue("an empty quoted line kept a trailing space", out.contains("\n>\n"))
        assertFalse(out, out.contains("> two\n> "))

        assertEquals("", ReplyDraft.quoted("", headers()))
        assertEquals("", ReplyDraft.quoted("   \n\n ", headers()))
    }

    /**
     * The reply is from the account it is sent from, with that account's
     * name — not the name of whoever is being replied to.
     */
    @Test
    fun `the reply is from the account`() {
        val draft = ReplyDraft.make(headers(), me)
        assertEquals("me@example.com", draft.from)
        assertEquals("Me", draft.fromName)
        assertEquals(listOf("Ada <ada@example.com>"), draft.to)
    }

    /**
     * Reply-all copies everyone who was already on it, and neither the
     * author nor the person being answered — the first is the mistake
     * everybody notices and nobody can undo.
     */
    @Test
    fun `reply all copies the others and not me`() {
        val h = headers().copy(
            to = "Me <me@example.com>, Ada <ada@example.com>",
            cc = "bob@example.com",
        )
        val draft = ReplyDraft.make(h, me, all = true)
        assertEquals(listOf("Ada <ada@example.com>"), draft.to)
        assertEquals(listOf("bob@example.com"), draft.cc)
    }

    /**
     * Off by default, and a separate action rather than a setting:
     * whether the rest of a list should see an answer is a decision per
     * message, and a client that decides it once decides it wrong half
     * the time.
     */
    @Test
    fun `an ordinary reply copies nobody`() {
        val h = headers().copy(to = "a@example.com, b@example.com", cc = "c@example.com")
        assertTrue(ReplyDraft.make(h, me).cc.isEmpty())
    }
}
