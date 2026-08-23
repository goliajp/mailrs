package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Where a provider's servers are, and what it calls the secret.
 *
 * Its failure modes are quiet: a wrong host sends somebody's password
 * to the wrong machine, and a wrong label for the secret sends them
 * looking for a password they never set.
 */
class MailProviderTest {
    @Test
    fun `a known address needs nothing else`() {
        val p = MailProvider.forAddress("someone@gmail.com")
        assertEquals("imap.gmail.com", p?.imapHost)
        assertEquals("smtp.gmail.com", p?.smtpHost)
        assertEquals(MailProvider.AuthKind.OAUTH2, p?.auth)
    }

    // A suffix match would send somebody's password to Google.
    @Test
    fun `a lookalike domain is not the provider`() {
        assertNull(MailProvider.forDomain("notgmail.com"))
        assertNull(MailProvider.forDomain("gmail.com.evil.example"))
    }

    @Test
    fun `the case of what somebody typed does not matter`() {
        assertEquals("Gmail", MailProvider.forAddress("Someone@GMail.COM")?.label)
    }

    // A plus-address and a name with an @ in it: the domain is what
    // follows the **last** @.
    @Test
    fun `the domain is what follows the last at`() {
        assertEquals("QQ", MailProvider.forAddress("first+tag@qq.com")?.label)
        assertEquals("网易 163", MailProvider.forAddress("odd@name@163.com")?.label)
    }

    // The provider's own word, because a person will be looking for
    // exactly that string in its settings.
    @Test
    fun `a provider that wants a code says what it calls it`() {
        assertEquals("授权码", MailProvider.forDomain("qq.com")?.secretHelp?.what)
        assertEquals(
            "app-specific password",
            MailProvider.forDomain("icloud.com")?.secretHelp?.what,
        )
        // Gmail refuses passwords entirely, so there is no code to make.
        assertNull(MailProvider.forDomain("gmail.com")?.secretHelp)
    }

    // Reading Gmail's All Mail doubles every message in the mailbox.
    @Test
    fun `a view holding everything is left alone`() {
        assertTrue(
            MailProvider.forDomain("gmail.com")!!.skipFolders.contains("[Gmail]/All Mail"),
        )
    }

    @Test
    fun `an unknown domain still gets a starting point`() {
        val g = MailProvider.guess("internal.example.jp")
        assertEquals("imap.internal.example.jp", g.imapHost)
        assertEquals(MailProvider.AuthKind.PASSWORD, g.auth)
    }

    @Test
    fun `the other names for the same provider work`() {
        assertEquals("Gmail", MailProvider.forDomain("googlemail.com")?.label)
        assertEquals("Outlook", MailProvider.forDomain("hotmail.co.jp")?.label)
        assertEquals("QQ", MailProvider.forDomain("foxmail.com")?.label)
        assertEquals("网易 163", MailProvider.forDomain("126.com")?.label)
    }

    // A table entry with a plaintext port is a table entry that leaks
    // a password.
    @Test
    fun `no entry offers a plaintext port`() {
        for ((domain, p) in MailProvider.table) {
            assertEquals("$domain imap", 993, p.imapPort)
            assertTrue("$domain smtp ${p.smtpPort}", p.smtpPort == 465 || p.smtpPort == 587)
        }
    }
}
