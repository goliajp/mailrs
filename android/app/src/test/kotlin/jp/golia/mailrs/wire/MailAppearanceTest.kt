package jp.golia.mailrs.wire

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The two questions asked of a message body before it is painted, both
 * ported from iOS so the clients cannot answer them differently.
 */
class MailAppearanceTest {

    /** A paragraph and a link declares nothing, so it may go dark. */
    @Test
    fun plain_mail_follows_the_app() {
        assertTrue(MailAppearance.followsAppTheme("<p>Lunch at one?</p>"))
        assertTrue(MailAppearance.followsAppTheme("hello"))
    }

    /**
     * A message that styles itself keeps white paper. Black text on a
     * dark background is worse than a bright rectangle, which is the
     * whole reason this question exists.
     */
    @Test
    fun mail_that_declares_a_colour_keeps_white_paper() {
        assertFalse(MailAppearance.followsAppTheme("<body bgcolor=\"#fff\">x</body>"))
        assertFalse(MailAppearance.followsAppTheme("<font color=red>x</font>"))
        assertFalse(MailAppearance.followsAppTheme("<div style=\"background:#eee\">x</div>"))
        assertFalse(MailAppearance.followsAppTheme("<span style=\"color:#111\">x</span>"))
    }

    /**
     * **A border colour says nothing about whether the text is
     * legible.** This is the one subtle case: a naive `contains("color:")`
     * puts every message with a ruled table onto white paper for no
     * reason.
     */
    @Test
    fun a_border_colour_is_not_a_text_colour() {
        assertTrue(MailAppearance.followsAppTheme("<td style=\"border-color:#ddd\">x</td>"))
        assertTrue(MailAppearance.followsAppTheme("<td style=\"outline-color:#ddd\">x</td>"))
    }

    /** Case is not a signal: `COLOR:` is a declared colour too. */
    @Test
    fun the_test_is_case_insensitive() {
        assertFalse(MailAppearance.followsAppTheme("<SPAN STYLE=\"COLOR:#111\">x</SPAN>"))
        assertFalse(MailAppearance.followsAppTheme("<BODY BGCOLOR=white>x</BODY>"))
    }

    /** A declared colour after a border colour still counts. */
    @Test
    fun a_real_colour_after_a_border_colour_is_found() {
        assertFalse(
            MailAppearance.followsAppTheme(
                "<td style=\"border-color:#ddd\"><span style=\"color:#111\">x</span></td>",
            ),
        )
    }
}

class RemoteContentTest {

    @Test
    fun a_message_with_no_references_loads_nothing() {
        assertFalse(RemoteContent.hasRemoteReferences("<p>Lunch at one?</p>"))
        assertFalse(RemoteContent.hasRemoteReferences("<img src=\"cid:logo\">"))
        assertFalse(RemoteContent.hasRemoteReferences("<img src=\"data:image/png;base64,AA\">"))
    }

    /**
     * Generous on purpose: a missed reference is a silent leak, a false
     * positive is one banner on a message with nothing to load.
     */
    @Test
    fun every_shape_of_remote_reference_counts() {
        assertTrue(RemoteContent.hasRemoteReferences("<img src=\"https://t.example/a.gif\">"))
        assertTrue(RemoteContent.hasRemoteReferences("<img src='http://t.example/a.gif'>"))
        assertTrue(RemoteContent.hasRemoteReferences("<img src=http://t.example/a.gif>"))
        assertTrue(RemoteContent.hasRemoteReferences("<img src=\"//t.example/a.gif\">"))
        assertTrue(RemoteContent.hasRemoteReferences("<td background=\"https://t.example/b.png\">"))
        assertTrue(RemoteContent.hasRemoteReferences("<div style=\"background:url(https://t.example/c)\">"))
        assertTrue(RemoteContent.hasRemoteReferences("<div style=\"background:url('//t.example/c')\">"))
    }

    /** A tracking pixel and a logo report identically, so both count. */
    @Test
    fun a_logo_is_a_beacon_too() {
        assertTrue(RemoteContent.hasRemoteReferences("<img src=\"https://brand.example/logo.png\" width=200>"))
    }
}
