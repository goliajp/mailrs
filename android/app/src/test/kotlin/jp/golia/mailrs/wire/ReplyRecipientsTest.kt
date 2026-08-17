package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The reply rules, which are the same on all three clients or one of
 * them quietly drops a cc.
 */
class ReplyRecipientsTest {

    @Test
    fun a_reply_goes_to_the_sender() {
        assertEquals(listOf("a@x.test"), ReplyRecipients.reply("Ann <a@x.test>"))
        assertEquals(listOf("a@x.test"), ReplyRecipients.reply("a@x.test"))
    }

    /**
     * **A reply must not arrive addressed back at the person sending
     * it.** That is the rule the whole function exists for, so it is
     * asserted first and by name.
     */
    @Test
    fun reply_all_drops_me() {
        val out = ReplyRecipients.replyAll(
            sender = "Ann <a@x.test>",
            recipients = "me@golia.jp, Bob <b@x.test>",
            myAddress = "me@golia.jp",
        )
        assertEquals(listOf("a@x.test", "b@x.test"), out)
    }

    @Test
    fun reply_all_keeps_the_sender_first_and_collapses_duplicates() {
        val out = ReplyRecipients.replyAll(
            sender = "Ann <a@x.test>",
            recipients = "Bob <b@x.test>; a@x.test, b@x.test",
            myAddress = "me@golia.jp",
        )
        assertEquals(listOf("a@x.test", "b@x.test"), out)
    }

    @Test
    fun my_address_is_matched_however_it_is_cased() {
        val out = ReplyRecipients.replyAll(
            sender = "Ann <a@x.test>",
            recipients = "ME@Golia.JP",
            myAddress = "me@golia.jp",
        )
        assertEquals(listOf("a@x.test"), out)
    }

    /** `RE: x` must not become `Re: RE: x`. */
    @Test
    fun a_subject_gains_one_prefix_at_most() {
        assertEquals("Re: Lunch", ReplyRecipients.subject("Lunch"))
        assertEquals("Re: Lunch", ReplyRecipients.subject("Re: Lunch"))
        assertEquals("RE: Lunch", ReplyRecipients.subject("RE: Lunch"))
        assertEquals("Fwd: Lunch", ReplyRecipients.subject("Lunch", forwarding = true))
        assertEquals("Fwd: Lunch", ReplyRecipients.subject("Fwd: Lunch", forwarding = true))
    }

    /** An empty subject gets the bare prefix, not "Re: ". */
    @Test
    fun an_empty_subject_is_just_the_prefix() {
        assertEquals("Re:", ReplyRecipients.subject(""))
    }

    @Test
    fun the_quote_carries_the_original_indented() {
        val q = ReplyRecipients.quote("Ann <a@x.test>", 1_700_000_000, "line one\nline two")
        assert(q.contains("> line one")) { q }
        assert(q.contains("> line two")) { q }
        assert(q.contains("Ann")) { q }
    }
}
