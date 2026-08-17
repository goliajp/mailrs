package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Signing a message, and knowing when not to. */
class MailSignatureTest {

    /** The trailing space is part of RFC 3676's separator. */
    @Test
    fun the_separator_is_two_hyphens_and_a_space() {
        assertEquals("-- ", MailSignature.SEPARATOR)
    }

    @Test
    fun a_signature_goes_under_a_separator() {
        assertEquals(
            "Hello\n\n-- \nLi Hao",
            MailSignature.append("Hello", "Li Hao"),
        )
    }

    /** An empty signature leaves the body alone, separator and all. */
    @Test
    fun nothing_is_appended_when_there_is_no_signature() {
        assertEquals("Hello", MailSignature.append("Hello", ""))
        assertEquals("Hello", MailSignature.append("Hello", "   \n "))
    }

    /**
     * **A body that already carries one is left alone.** A reply quotes
     * the original beneath what was typed, and a second signature
     * between the two reads as though the sender signed the other
     * person's message.
     */
    @Test
    fun a_body_that_already_carries_a_signature_is_not_signed_twice() {
        val reply = "Sure.\n\n-- \nLi Hao\n\nOn Monday, Alice wrote:\n> hello"
        assertEquals(reply, MailSignature.append(reply, "Li Hao"))
    }

    /** CRLF is what a Windows client sends; the separator still counts. */
    @Test
    fun a_separator_is_found_in_crlf_text() {
        assertTrue(MailSignature.carriesOne("Sure.\r\n\r\n-- \r\nLi Hao"))
        assertFalse(MailSignature.carriesOne("Sure.\r\nno separator here"))
    }

    /** An empty body gets the signature without a leading blank. */
    @Test
    fun an_empty_body_is_just_the_signature() {
        assertEquals("-- \nLi Hao", MailSignature.append("   ", "Li Hao"))
    }

    /**
     * The default one, not the first: somebody with two signatures has
     * said which is theirs, and picking the first would sign work mail
     * "Sent from a phone" forever.
     */
    @Test
    fun the_default_signature_wins_over_the_first() {
        val signatures = listOf(
            Wire.Signature(id = 1, name = "Short", textContent = "Sent from a phone"),
            Wire.Signature(id = 2, name = "Work", textContent = "Li Hao", isDefault = true),
        )
        assertEquals("Li Hao", MailSignature.preferred(signatures))
        assertEquals("", MailSignature.preferred(emptyList()))
    }
}
