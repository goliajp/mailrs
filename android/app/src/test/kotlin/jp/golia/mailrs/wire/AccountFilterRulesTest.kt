package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * Narrowing the list to some of the connected mailboxes.
 *
 * The same rule runs on the web and on iOS, and each has this test:
 * a filter that behaves differently on a phone is a filter nobody
 * trusts.
 */
class AccountFilterRulesTest {
    private val all = listOf("", "acc_1", "acc_2")

    @Test
    fun `starts with everything and unticking one narrows it`() {
        assertEquals(listOf("", "acc_2"), toggledAccounts(null, all, "acc_1"))
    }

    // Back to everything is the parameter absent, not every id in it:
    // the two narrow to the same set, and only one of them is legible.
    @Test
    fun `ticking the last one back returns to no filter at all`() {
        assertNull(toggledAccounts(listOf("", "acc_2"), all, "acc_1"))
    }

    // A list narrowed to no accounts is a blank screen whose only way
    // back is the control that produced it.
    @Test
    fun `refuses to untick the last one`() {
        assertEquals(listOf("acc_1"), toggledAccounts(listOf("acc_1"), all, "acc_1"))
    }

    @Test
    fun `says what it is doing`() {
        assertEquals("All accounts", filterLabel(null, all))
        assertEquals("All accounts", filterLabel(all, all))
        assertEquals("1 of 3 accounts", filterLabel(listOf("acc_1"), all))
    }

    // The empty id is this deployment's own mail — a row like the
    // rest, so it can be switched off too.
    @Test
    fun `this server is a row like the others`() {
        val rows = filterRows(
            "me@golia.jp",
            listOf(ExternalAccount(id = "a1", email = "x@gmail.com", displayName = "Work")),
        )
        assertEquals("", rows[0].id)
        assertEquals("me@golia.jp", rows[0].label)
        assertEquals("Work", rows[1].label)
    }
}
