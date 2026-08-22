package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Narrowing the one list to some of the connected mailboxes. */
class AccountFilterTest {
    private fun param(axes: MailListAxes): String? =
        axes.query().split("&").firstOrNull { it.startsWith("accounts=") }

    /** No filter sends no parameter — the shape the server reads as
     * "every account". */
    @Test
    fun `no filter sends nothing`() {
        assertFalse(MailListAxes().query().contains("accounts"))
    }

    /**
     * The one that would show the wrong list: unticking every account
     * and being served all of it, because an empty selection was
     * collapsed into "no filter".
     */
    @Test
    fun `unticking everything is not the same as no filter`() {
        assertEquals("accounts=", param(MailListAxes(accounts = emptyList())))
    }

    @Test
    fun `several accounts are comma separated`() {
        assertEquals("accounts=ext_a,ext_b", param(MailListAxes(accounts = listOf("ext_a", "ext_b"))))
    }

    /** This server's own mail is the empty id, so a selection holding
     * it is not an empty selection. */
    @Test
    fun `this servers own mail can be named`() {
        assertEquals("accounts=,ext_a", param(MailListAxes(accounts = listOf("", "ext_a"))))
        assertTrue(param(MailListAxes(accounts = listOf("", "ext_a")))!!.length > "accounts=".length)
    }
}
