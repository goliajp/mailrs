package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The answer a sign-in to an account whose password was cleared gets. */
class PasswordClearedTest {
    @Test
    fun `the server's contact becomes whom to ask`() {
        val body = """{"error":"password_cleared","contact":{"name":"Li Hao","address":"lihao@golia.jp"}}"""
        assertEquals("Li Hao (lihao@golia.jp)", clearedContact(body))
    }

    @Test
    fun `half a contact is used as it is, none is an administrator`() {
        assertEquals("lihao@golia.jp", contactText("", "lihao@golia.jp"))
        assertEquals("Li Hao", contactText("Li Hao", ""))
        assertEquals("an administrator", contactText("", ""))
        assertEquals("an administrator", clearedContact("""{"error":"password_cleared"}"""))
    }

    @Test
    fun `any other refusal is not one`() {
        assertNull(clearedContact(""))
        assertNull(clearedContact("""{"error":"invalid TOTP code"}"""))
        assertNull(clearedContact("not json"))
    }
}
