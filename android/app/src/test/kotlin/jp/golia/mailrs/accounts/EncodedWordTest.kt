package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * RFC 2047 encoded words.
 *
 * Without this every Japanese or Chinese subject in the list is a run
 * of `=?UTF-8?B?` gibberish — the most visible way a mail client can
 * look broken.
 */
class EncodedWordTest {
    @Test
    fun `a base64 word becomes text`() {
        assertEquals("会議の件", EncodedWord.decode("=?UTF-8?B?5Lya6K2w44Gu5Lu2?="))
    }

    @Test
    fun `a quoted printable word becomes text`() {
        assertEquals("café", EncodedWord.decode("=?UTF-8?Q?caf=C3=A9?="))
        // `_` is a space in Q-encoding, not an underscore.
        assertEquals("two words", EncodedWord.decode("=?UTF-8?Q?two_words?="))
    }

    // A subject is often half encoded and half not, and re-encoding the
    // plain half would corrupt it.
    @Test
    fun `plain text around a word is untouched`() {
        assertEquals("Re: 会議 (2)", EncodedWord.decode("Re: =?UTF-8?B?5Lya6K2w?= (2)"))
    }

    // **RFC 2047 6.2.** Whitespace *between two encoded words* is there
    // so they can be folded — it is not part of the text. A decoder
    // that keeps it puts a space in the middle of every long CJK
    // subject.
    @Test
    fun `the gap between two words is not text`() {
        assertEquals(
            "会議の件",
            EncodedWord.decode("=?UTF-8?B?5Lya6K2w?= =?UTF-8?B?44Gu5Lu2?="),
        )
    }

    // But a gap between a word and plain text **is** text.
    @Test
    fun `the gap before plain text survives`() {
        assertEquals("会議 today", EncodedWord.decode("=?UTF-8?B?5Lya6K2w?= today"))
    }

    @Test
    fun `an unpadded word still decodes`() {
        assertEquals("abc", EncodedWord.decode("=?UTF-8?B?YWJj?="))
    }

    // Mojibake somebody can report beats text this app invented.
    @Test
    fun `an unknown charset is left alone`() {
        val s = "=?X-MADE-UP?B?YWJj?="
        assertEquals(s, EncodedWord.decode(s))
    }

    @Test
    fun `something that is not a word is left alone`() {
        assertEquals("plain subject", EncodedWord.decode("plain subject"))
        assertEquals("=? not a word", EncodedWord.decode("=? not a word"))
        assertEquals("", EncodedWord.decode(""))
    }
}
