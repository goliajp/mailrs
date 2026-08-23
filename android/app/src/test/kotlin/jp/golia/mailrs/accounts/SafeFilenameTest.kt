package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * A name from a stranger, made safe to write to disk.
 *
 * An attachment's filename is attacker-controlled: it arrives in a
 * header written by whoever sent the message. This has been a real bug
 * in real mail clients more than once.
 */
class SafeFilenameTest {
    /** An ordinary name is left exactly as it was. */
    @Test
    fun `an ordinary name is untouched`() {
        assertEquals("report 2025.pdf", SafeFilename.of("report 2025.pdf"))
        assertEquals("日本.pdf", SafeFilename.of("日本.pdf"))
    }

    /**
     * **The property is that the result never escapes the directory**
     * — not that a particular string comes out. Collapsing
     * `../../x.xml` to `x.xml` is safe and keeps the name the sender
     * meant; refusing outright would be safe too and would throw the
     * name away. Asserting the property means either can be chosen and
     * neither can drift into being unsafe.
     */
    @Test
    fun `nothing escapes the directory`() {
        val attacks = listOf(
            "../../../shared_prefs/auth.xml",
            "/etc/passwd",
            "..\\windows\\system32",
            "..",
            ".",
            "a/../../b",
        )
        // Canonicalised on both sides: `/tmp` is a symlink to
        // `/private/tmp` on macOS, so comparing a resolved path against
        // an unresolved prefix fails on a file that is where it should
        // be. The measurement was wrong, not the thing measured.
        val box = java.io.File("/tmp/box").canonicalPath
        for (attack in attacks) {
            val safe = SafeFilename.of(attack)
            assertFalse(attack, safe.contains('/'))
            assertFalse(attack, safe.contains('\\'))
            assertFalse(attack, safe == "." || safe == "..")
            val written = java.io.File(java.io.File(box), safe).canonicalPath
            assertTrue("$attack landed at $written", written.startsWith("$box/"))
        }
    }

    /** A directory part is dropped and the leaf is kept. */
    @Test
    fun `a directory part is dropped`() {
        assertEquals("photo.jpg", SafeFilename.of("holiday/photo.jpg"))
    }


    /**
     * The \u0000 that truncates a name in every C API underneath, and the
     * control characters that make a name unprintable.
     */
    @Test
    fun `control characters are removed`() {
        assertEquals("safe.pdf", SafeFilename.of("safe\u0000.pdf"))
        assertEquals("ab.txt", SafeFilename.of("a\u0009b\u007F.txt"))
        assertEquals("attachment", SafeFilename.of("\u0000\u0000"))
    }

    /**
     * A leading dot is **kept**.
     *
     * Stripping it would rename a `.gitignore` somebody attached on
     * purpose, and this file goes to the cache and straight to another
     * app through a content URI — nobody browses for it there.
     */
    @Test
    fun `a leading dot is kept`() {
        assertEquals(".bashrc", SafeFilename.of(".bashrc"))
        // `.` and `..` are still refused: those are directories.
        assertEquals("attachment", SafeFilename.of("."))
        assertEquals("attachment", SafeFilename.of(".."))
    }

    /** Nothing at all is the fallback, and the fallback is nameable. */
    @Test
    fun `an empty name becomes the fallback`() {
        assertEquals("attachment", SafeFilename.of(""))
        assertEquals("attachment", SafeFilename.of("   "))
        assertEquals("image.jpg", SafeFilename.of("", fallback = "image.jpg"))
    }

    /**
     * Shortened from the stem, never from the end: a name cut at the end
     * loses its extension, and a file the phone cannot tell the type of
     * is a file nothing will open.
     */
    @Test
    fun `a very long name keeps its extension`() {
        val long = "a".repeat(500) + ".pdf"
        val safe = SafeFilename.of(long)
        assertTrue(safe, safe.endsWith(".pdf"))
        assertTrue(safe, safe.toByteArray(Charsets.UTF_8).size <= 200)
    }

    /**
     * And never through a character: a name cut inside a multi-byte
     * sequence writes a filename with a replacement character in it.
     */
    @Test
    fun `a long multibyte name is not cut through a character`() {
        val long = "日".repeat(300) + ".pdf"
        val safe = SafeFilename.of(long)
        assertTrue(safe, safe.toByteArray(Charsets.UTF_8).size <= 200)
        assertFalse(safe, safe.contains('\uFFFD'))
        assertTrue(safe, safe.endsWith(".pdf"))
    }
}
