package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Test

/** The same cases as `ios/MailrsTests/SenderNameTests.swift`. */
class SenderNameTest {
    @Before
    fun englishWords() = TestTexts.install()

    @Test
    fun extracts_the_address_from_every_shape() {
        assertEquals("alice@example.com", SenderName.extractEmail("Alice Smith <alice@example.com>"))
        assertEquals("bob@example.com", SenderName.extractEmail("<bob@example.com>"))
        assertEquals("charlie@example.com", SenderName.extractEmail("charlie@example.com"))
        assertEquals("not-an-email", SenderName.extractEmail("not-an-email"))
        assertEquals("dave@example.com", SenderName.extractEmail("\"Dave Doe\" <dave@example.com>"))
    }

    @Test
    fun extracts_the_display_name() {
        assertEquals("Alice Smith", SenderName.extractName("Alice Smith <alice@example.com>"))
        assertEquals("Bob Jones", SenderName.extractName("\"Bob Jones\" <bob@example.com>"))
        assertEquals("Dave", SenderName.extractName("  Dave  <dave@example.com>"))
    }

    @Test
    fun bare_addresses_show_the_local_part() {
        assertEquals("charlie", SenderName.extractName("charlie@example.com"))
        assertEquals("notanemail", SenderName.extractName("notanemail"))
    }

    @Test
    fun machine_addresses_show_the_brand() {
        assertEquals("Cloudflare", SenderName.extractName("bounce-12345-abcdef@notify.cloudflare.com"))
        assertEquals("Freee", SenderName.extractName("msprvs1=18274xyzw=k@em8742.bsm.freee.work"))
    }

    @Test
    fun decodes_encoded_word_names() {
        val encoded = "=?UTF-8?B?5bGx55Sw5aSq6YOO?= <yamada@example.co.jp>"
        assertEquals("山田太郎", SenderName.extractName(encoded))
        assertEquals("yamada@example.co.jp", SenderName.extractEmail(encoded))
    }

    @Test
    fun domain_label_skips_secondary_tlds() {
        assertEquals("example", SenderName.domainLabel("example.co.jp"))
        assertEquals("cloudflare", SenderName.domainLabel("notify.cloudflare.com"))
        assertEquals("localhost", SenderName.domainLabel("localhost"))
    }

    @Test
    fun the_face_is_the_other_participant() {
        assertEquals(
            "Alice Smith",
            SenderName.rowFace(listOf("me@golia.jp", "Alice Smith <alice@example.com>"), "me@golia.jp"),
        )
    }

    @Test
    fun own_reply_does_not_steal_the_face() {
        assertEquals("bob", SenderName.rowFace(listOf("Me <ME@Golia.jp>", "bob@example.com"), "me@golia.jp"))
    }

    @Test
    fun self_only_thread_says_me() {
        assertEquals("Me", SenderName.rowFace(listOf("me@golia.jp"), "me@golia.jp"))
    }

    @Test
    fun others_leave_the_reader_out() {
        val many = (0 until 40).map { "person$it@example.com" }
        assertEquals(39, SenderName.others(many, "me@golia.jp").size - 1)
        assertEquals(1, SenderName.others(listOf("Me <me@golia.jp>", "bob@example.com"), "me@golia.jp").size)
    }

    /** The reading page's sender line goes through this; it showed `keiri@example.co.jp`. */
    @Test
    fun the_readable_name_of_a_bare_address_is_its_local_part() {
        assertEquals("keiri", SenderIdentity.readableName("keiri@example.co.jp"))
        assertEquals("spoofed", SenderIdentity.readableName("spoofed@example.com"))
        assertEquals("Alice Smith", SenderIdentity.readableName("Alice Smith <alice@example.com>"))
    }
}
