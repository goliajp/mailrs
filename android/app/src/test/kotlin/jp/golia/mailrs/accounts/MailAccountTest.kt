package jp.golia.mailrs.accounts

import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** A mailbox somewhere else, as this app holds it. */
class MailAccountTest {
    // One address, one row — adding the same account twice must be the
    // same row rather than two, and a stored credential has to survive
    // a list rebuilt from scratch.
    @Test
    fun `the same address is always the same id`() {
        assertEquals(MailAccount.idFor("me@gmail.com"), MailAccount.idFor("me@gmail.com"))
        assertEquals(MailAccount.idFor("Me@Gmail.com"), MailAccount.idFor("me@gmail.com"))
        assertTrue(MailAccount.idFor("me@gmail.com") != MailAccount.idFor("you@gmail.com"))
    }

    @Test
    fun `a known provider fills itself in`() {
        val a = MailAccount.make("someone@qq.com")
        assertEquals("imap.qq.com", a.imapHost)
        assertEquals(465, a.smtpPort)
        assertEquals(MailProvider.AuthKind.APP_PASSWORD, a.auth)
        assertEquals("qq", a.provider)
    }

    @Test
    fun `an unknown domain is marked custom`() {
        val a = MailAccount.make("me@internal.example.jp")
        assertEquals("custom", a.provider)
        assertEquals("imap.internal.example.jp", a.imapHost)
    }

    // The row is the thing that gets logged, encoded and shown. It must
    // not be the thing that carries a password.
    @Test
    fun `the row holds no secret`() {
        val a = MailAccount.make("me@qq.com")
        val json = Json.encodeToString(MailAccount.serializer(), a)
        assertFalse(json.contains("hunter2"))
        // And no field is named for one. `auth` names the **kind** of
        // credential the server wants, which the row is right to hold.
        for (name in listOf("\"password\"", "\"secret\"", "\"token\"", "\"credential\"")) {
            assertFalse("the row has a field called $name", json.contains("$name:"))
        }
    }

    // Said before spending thirty seconds finding out that a blank host
    // does not resolve.
    @Test
    fun `what is missing is said in words somebody can act on`() {
        assertNull(MailAccount.make("me@example.jp").problem)
        assertEquals(
            "The incoming server needs a name",
            MailAccount.make("me@example.jp").copy(imapHost = "").problem,
        )
        assertEquals(
            "That is not an email address",
            MailAccount.make("not-an-address").problem,
        )
    }

    @Test
    fun `a row with no name shows its address`() {
        val a = MailAccount.make("me@qq.com")
        assertEquals("me@qq.com", a.title)
        assertNull(a.subtitle)
        val named = a.copy(displayName = "Work")
        assertEquals("Work", named.title)
        assertEquals("me@qq.com", named.subtitle)
    }

    @Test
    fun `the login name falls back to the address`() {
        val a = MailAccount.make("me@example.jp")
        assertEquals("me@example.jp", a.loginName)
        assertEquals("me", a.copy(login = "me").loginName)
    }

    // The same account is the same colour on every launch, and two
    // accounts are unlikely to collide.
    @Test
    fun `a colour is stable and comes from the palette`() {
        val id = MailAccount.idFor("me@qq.com")
        assertEquals(MailAccount.colourFor(id), MailAccount.colourFor(id))
        assertTrue(MailAccount.palette.contains(MailAccount.colourFor(id)))
    }
}
