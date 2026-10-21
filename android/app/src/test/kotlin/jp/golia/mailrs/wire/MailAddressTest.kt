package jp.golia.mailrs.wire

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MailAddressTest {
    @Test
    fun `an address has a name, one at sign and a dotted domain`() {
        for (ok in listOf("qa@golia.jp", "a.b+tag@mail.example.co.jp")) {
            assertTrue(ok, looksLikeAddress(ok))
        }
        for (bad in listOf("", "qa", "qa@", "@golia.jp", "qa@golia", "qa@golia.", "qa@.jp", "a@b@c.jp", "qa @golia.jp", "去啊@我俩.")) {
            assertFalse(bad, looksLikeAddress(bad))
        }
    }
}
