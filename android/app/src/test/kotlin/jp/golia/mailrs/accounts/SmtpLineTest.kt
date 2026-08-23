package jp.golia.mailrs.accounts

import java.util.Base64
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Reading what an SMTP server says, and the two AUTH payloads. */
class SmtpLineTest {
    // The fourth character decides whether more lines follow. Getting
    // it wrong reads the next command's reply as this one's.
    @Test
    fun `a continuation is told from an ending`() {
        assertEquals(true, Smtp.reply("250-STARTTLS")?.more)
        assertEquals(false, Smtp.reply("250 OK")?.more)
        assertEquals(false, Smtp.reply("250")?.more)
    }

    @Test
    fun `a code says whether to try again`() {
        assertEquals(false, Smtp.reply("451 try later")?.isPermanent)
        assertEquals(true, Smtp.reply("550 no such user")?.isPermanent)
        assertEquals(true, Smtp.reply("250 OK")?.isPositive)
        assertEquals(false, Smtp.reply("550 no")?.isPositive)
    }

    @Test
    fun `a line that is not a reply is not guessed at`() {
        assertNull(Smtp.reply("hello"))
        assertNull(Smtp.reply("250x OK"))
        assertNull(Smtp.reply(""))
    }

    // NUL separators, not spaces. Spaces authenticate as nobody and the
    // server answers with what reads as a wrong password.
    @Test
    fun `auth plain is nul separated`() {
        val raw = Base64.getDecoder().decode(Smtp.authPlain("me@x.com", "hunter2"))
        val want = byteArrayOf(0) + "me@x.com".toByteArray() +
            byteArrayOf(0) + "hunter2".toByteArray()
        assertArrayEquals(want, raw)
    }

    // An access token is not a password, and the difference is the
    // whole point.
    @Test
    fun `an access token is not sent as a password`() {
        val plain = Smtp.authPlain("me@gmail.com", "ya29.token")
        val xoauth = Smtp.authXOAuth2("me@gmail.com", "ya29.token")
        assertTrue(plain != xoauth)
        val raw = Base64.getDecoder().decode(xoauth)
        assertEquals(
            "user=me@gmail.com\u0001auth=Bearer ya29.token\u0001\u0001",
            String(raw),
        )
        assertFalse(
            "a NUL here is the AUTH PLAIN shape, which is refused",
            raw.contains(0),
        )
    }

    // A body line beginning with `.` would end the DATA block,
    // truncating the message at that line.
    @Test
    fun `a line starting with a dot does not end the message`() {
        assertEquals("first\r\n..hidden\r\nlast", Smtp.dotStuffed("first\n.hidden\nlast"))
    }

    @Test
    fun `an ordinary body is only given crlf`() {
        assertEquals("a\r\nb", Smtp.dotStuffed("a\nb"))
        assertEquals("a\r\nb", Smtp.dotStuffed("a\r\nb"))
    }

    // A dot in the middle of a line is not a terminator and must not be
    // doubled — that would corrupt the text.
    @Test
    fun `a dot inside a line is left alone`() {
        assertEquals("see fig. 1", Smtp.dotStuffed("see fig. 1"))
    }

    @Test
    fun `a refused credential is told from a server having a bad day`() {
        assertTrue(Smtp.isAuthenticationFailure(535, "5.7.8 nope"))
        assertTrue(Smtp.isAuthenticationFailure(501, "Username and Password not accepted"))
        assertFalse(Smtp.isAuthenticationFailure(451, "Temporary system problem"))
    }
}
