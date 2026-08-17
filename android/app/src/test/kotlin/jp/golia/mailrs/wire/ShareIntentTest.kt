package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Test

/** What a `mailto:` link is asking for. */
class ShareIntentTest {

    @Test
    fun a_bare_address_is_the_recipient() {
        assertEquals("a@x.test", ShareIntent.mailto("mailto:a@x.test").to)
    }

    /**
     * A subject that arrives still percent-encoded looks like the
     * sender typed `%20`, which is the quiet way this goes wrong.
     */
    @Test
    fun the_query_is_decoded() {
        val m = ShareIntent.mailto("mailto:a@x.test?subject=Hello%20there&body=Line%20one")
        assertEquals("Hello there", m.subject)
        assertEquals("Line one", m.body)
    }

    /**
     * **`+` is a space only in form encoding, and this is not a form.**
     * `a+tag@example.com` is a common address and must survive intact.
     */
    @Test
    fun a_plus_in_an_address_is_a_plus() {
        assertEquals("a+tag@x.test", ShareIntent.mailto("mailto:a+tag@x.test").to)
    }

    @Test
    fun cc_and_bcc_are_carried() {
        val m = ShareIntent.mailto("mailto:a@x.test?cc=b@x.test&bcc=c@x.test")
        assertEquals("b@x.test", m.cc)
        assertEquals("c@x.test", m.bcc)
    }

    /**
     * A `to=` in the query adds to the address the link led with rather
     * than replacing it — RFC 6068 allows both places, and overwriting
     * drops the addressee.
     */
    @Test
    fun a_to_in_the_query_adds_rather_than_replaces() {
        assertEquals("a@x.test, b@x.test", ShareIntent.mailto("mailto:a@x.test?to=b@x.test").to)
    }

    @Test
    fun an_empty_mailto_asks_for_an_empty_composer() {
        assertEquals(ShareIntent.Mailto(), ShareIntent.mailto("mailto:"))
    }

    /** Unknown parameters are ignored, not treated as a body. */
    @Test
    fun an_unknown_parameter_is_dropped() {
        val m = ShareIntent.mailto("mailto:a@x.test?in-reply-to=%3Cx%40y%3E&subject=Re")
        assertEquals("Re", m.subject)
        assertEquals("", m.body)
    }
}
