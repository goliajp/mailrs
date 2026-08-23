package jp.golia.mailrs.accounts

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The byte-preserving reader, and getting back out of it.
 *
 * This is what stands between a Shift_JIS message and a screen of
 * replacement characters: the socket may not decide what a message says
 * before the message has said what it is.
 */
class WireTest {
    /** Every byte value survives the trip, including the ones no text
     *  encoding would accept. */
    @Test
    fun `every byte survives the round trip`() {
        val all = ByteArray(256) { it.toByte() }
        val asRead = String(all, Charsets.ISO_8859_1)
        assertArrayEquals(all, Wire.bytes(asRead))
    }

    /** A UTF-8 body read as latin-1 is recovered exactly. */
    @Test
    fun `utf8 content is recovered`() {
        val original = "café — 日本語"
        val asRead = String(original.toByteArray(Charsets.UTF_8), Charsets.ISO_8859_1)
        assertEquals(original, Wire.utf8(asRead))
    }

    /**
     * Bytes that are not UTF-8 keep what they were rather than becoming
     * replacement characters — a latin-1 folder name should read as
     * latin-1.
     */
    @Test
    fun `non utf8 bytes are left as they were`() {
        val asRead = String(byteArrayOf(0x63, 0x61, 0x66, 0xE9.toByte()), Charsets.ISO_8859_1)
        assertEquals("café", Wire.utf8(asRead))
    }

    /** ASCII, which is nearly all of a mail session, is untouched. */
    @Test
    fun `ascii is unchanged either way`() {
        val line = "a1 OK [READ-WRITE] SELECT completed"
        assertEquals(line, Wire.utf8(line))
        assertArrayEquals(line.toByteArray(Charsets.US_ASCII), Wire.bytes(line))
    }
}
