package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Test

/** The words somebody reads when connecting fails. */
class AccountConnectionTest {
    // The bracket is for programs.
    @Test
    fun `the response code is not shown to a person`() {
        assertEquals(
            "Invalid credentials",
            AccountConnection.readable("[AUTHENTICATIONFAILED] Invalid credentials"),
        )
        assertEquals(
            "Please use an app password",
            AccountConnection.readable("[ALERT] Please use an app password"),
        )
    }

    // SMTP puts an enhanced status code in front for the same reason.
    @Test
    fun `the enhanced status code is not shown either`() {
        assertEquals(
            "Username and Password not accepted",
            AccountConnection.readable("5.7.8 Username and Password not accepted"),
        )
    }

    @Test
    fun `a plain reason is left alone`() {
        assertEquals("Try again later", AccountConnection.readable("Try again later"))
        assertEquals("[ONLYACODE]", AccountConnection.readable("[ONLYACODE]"))
    }

    // A version or an address is not a status code — stripping it
    // would eat the first word of the reason.
    @Test
    fun `something that is not a status code is not stripped`() {
        assertEquals(
            "1.2.3.4 is not permitted",
            AccountConnection.readable("1.2.3.4 is not permitted"),
        )
    }
}
