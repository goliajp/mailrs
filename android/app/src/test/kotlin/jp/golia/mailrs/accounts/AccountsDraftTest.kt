package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The rules the accounts screen applies before any request goes out. */
class AccountsDraftTest {
    private fun draft(ih: String, ip: String, sh: String, sp: String) =
        AccountsDraft(imapHost = ih, imapPort = ip, smtpHost = sh, smtpPort = sp)

    @Test
    fun `both servers go out when both are complete`() {
        val e = draft("imap.x.jp", "993", "smtp.x.jp", "465").endpoints()
        assertEquals("imap.x.jp", e?.imapHost)
        assertEquals(993, e?.imapPort)
        assertEquals(465, e?.smtpPort)
    }

    // A half-filled pair is refused here rather than by the server
    // thirty seconds later.
    @Test
    fun `a half-filled pair is refused`() {
        assertNull(draft("", "993", "smtp.x.jp", "465").endpoints())
        assertNull(draft("imap.x.jp", "", "smtp.x.jp", "465").endpoints())
        assertNull(draft("imap.x.jp", "993", "", "465").endpoints())
    }

    // `+993` parses as 993 on this platform, and that is not what
    // somebody typing a port means.
    @Test
    fun `only digits count as a port`() {
        for (p in listOf("+993", "99.5", "abc", "9 9", "0", "70000")) {
            assertNull("port $p was accepted", draft("h", p, "s", "465").endpoints())
        }
    }

    @Test
    fun `spaces around a port are somebody's paste, not a mistake`() {
        assertEquals(993, draft("h", " 993 ", "s", "465").endpoints()?.imapPort)
    }

    // Asking about "s", "so", "som" is three answers nobody asked for.
    @Test
    fun `a partial address is not looked up`() {
        for (partial in listOf("s", "so", "some@", "some@x")) {
            assertFalse(partial, AccountsDraft(address = partial).addressLooksComplete)
        }
        assertTrue(AccountsDraft(address = "some@x.jp").addressLooksComplete)
    }

    // The provider's own word, because a person will be looking for
    // exactly that string in its settings.
    @Test
    fun `the secret is labelled with the provider's own word`() {
        assertEquals("授权码", AccountsDraft(address = "someone@qq.com").secretLabel)
        assertEquals("Password", AccountsDraft(address = "me@internal.example.jp").secretLabel)
    }

    // An empty form is one somebody has to research; a filled one is
    // one they correct.
    @Test
    fun `the boxes open filled in`() {
        val d = AccountsDraft(address = "someone@qq.com").prefilled()
        assertEquals("imap.qq.com", d.imapHost)
        assertEquals("993", d.imapPort)
        assertEquals("smtp.qq.com", d.smtpHost)
    }

    // What somebody already typed is not overwritten by the guess.
    @Test
    fun `prefilling does not overwrite what was typed`() {
        val d = AccountsDraft(address = "someone@qq.com", imapHost = "mine.example.jp").prefilled()
        assertEquals("mine.example.jp", d.imapHost)
        assertEquals("993", d.imapPort)
    }

    @Test
    fun `a draft becomes an account, or says why not`() {
        val ok = AccountsDraft(address = "me@qq.com").account(0)
        assertEquals("imap.qq.com", ok.getOrNull()?.imapHost)

        val partial = AccountsDraft(address = "me@").account(0)
        assertTrue(partial.isFailure)

        val badManual = AccountsDraft(address = "me@x.jp", manual = true).account(0)
        assertEquals(
            "Both servers need a name and a port",
            badManual.exceptionOrNull()?.message,
        )
    }

    // A manual account is custom even when its address is a known
    // provider: somebody typing the servers in is saying the preset is
    // not what they want.
    @Test
    fun `typing the servers in makes it custom`() {
        val a = AccountsDraft(
            address = "someone@qq.com",
            manual = true,
            imapHost = "mine.example.jp", imapPort = "993",
            smtpHost = "out.example.jp", smtpPort = "465",
        ).account(0).getOrThrow()
        assertEquals("custom", a.provider)
        assertEquals("mine.example.jp", a.imapHost)
    }

    /**
     * JMAP has one endpoint and no separate outgoing server — it
     * submits over the same API. Demanding an SMTP host there asks for
     * something that does not exist, and somebody will type the
     * incoming one again to get past the form.
     */
    @Test
    fun `a jmap account needs no outgoing server`() {
        val draft = AccountsDraft(
            address = "me@example.com",
            manual = true,
            incoming = Incoming.JMAP,
            imapHost = "mail.example.com",
            imapPort = "443",
        )
        val endpoints = draft.endpoints()
        assertNotNull(endpoints)
        assertEquals("mail.example.com", endpoints!!.imapHost)
        assertTrue(draft.account(0).isSuccess)
        assertEquals(Incoming.JMAP, draft.account(0).getOrThrow().incoming)
    }

    /** An IMAP or POP3 account still needs both, because it has both. */
    @Test
    fun `every other kind still needs an outgoing server`() {
        val draft = AccountsDraft(
            address = "me@example.com",
            manual = true,
            incoming = Incoming.POP3,
            imapHost = "pop.example.com",
            imapPort = "995",
        )
        assertNull(draft.endpoints())
        assertTrue(draft.account(0).isFailure)
    }

    /**
     * The default port follows the protocol: 993 in a POP3 form is a
     * number somebody has to already know is wrong.
     */
    @Test
    fun `the prefilled port follows the protocol`() {
        val base = AccountsDraft(address = "me@example.com", manual = true)
        assertEquals("995", base.copy(incoming = Incoming.POP3).prefilled().imapPort)
        assertEquals("443", base.copy(incoming = Incoming.JMAP).prefilled().imapPort)
        assertEquals("993", base.copy(incoming = Incoming.IMAP).prefilled().imapPort)
    }

    /** What is typed is never overwritten by a default. */
    @Test
    fun `a typed port survives a protocol default`() {
        val draft = AccountsDraft(
            address = "me@example.com", manual = true,
            incoming = Incoming.POP3, imapPort = "1100",
        )
        assertEquals("1100", draft.prefilled().imapPort)
    }

}

/**
 * What an unknown domain is told.
 *
 * The guess is shown rather than described: saying "the usual names
 * are filled in below" while the boxes are shut is a sentence about
 * something the person cannot see, and if the guess is wrong they find
 * out thirty seconds later from a connection failure instead of now,
 * from reading it.
 */
class UnknownDomainTest {
    @Test
    fun `an unknown domain has a guess to show`() {
        val g = MailProvider.guess("internal.example.jp")
        assertEquals("imap.internal.example.jp", g.imapHost)
        assertEquals(993, g.imapPort)
        assertEquals("smtp.internal.example.jp", g.smtpHost)
        assertEquals(465, g.smtpPort)
    }

    // The guess and what the account is built with must agree — a
    // screen that shows one host and connects to another is worse than
    // one that shows nothing.
    @Test
    fun `what is shown is what will be tried`() {
        val shown = MailProvider.guess("internal.example.jp")
        val built = MailAccount.make("me@internal.example.jp")
        assertEquals(shown.imapHost, built.imapHost)
        assertEquals(shown.imapPort, built.imapPort)
        assertEquals(shown.smtpHost, built.smtpHost)
        assertEquals(shown.smtpPort, built.smtpPort)
    }
}
