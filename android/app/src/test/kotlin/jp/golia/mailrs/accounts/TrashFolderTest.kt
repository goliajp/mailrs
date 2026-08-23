package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Which folder a deleted message goes to. */
class TrashFolderTest {
    private fun folder(name: String, vararg attributes: String) =
        Imap.Untagged.ListFolder(name, attributes.toList())

    /**
     * The `\Trash` attribute is the server saying it in so many words,
     * and it is right regardless of language.
     */
    @Test
    fun `the special use attribute wins`() {
        val folders = listOf(
            folder("Trash"),
            folder("ゴミ箱", "\\HasNoChildren", "\\Trash"),
        )
        assertEquals("ゴミ箱", TrashFolder.pick(folders))
    }

    /** Servers that do not publish it fall back to the usual names. */
    @Test
    fun `the usual names are recognised`() {
        assertEquals("Trash", TrashFolder.pick(listOf(folder("INBOX"), folder("Trash"))))
        assertEquals(
            "Deleted Items",
            TrashFolder.pick(listOf(folder("INBOX"), folder("Deleted Items"))),
        )
        assertEquals(
            "Deleted Messages",
            TrashFolder.pick(listOf(folder("Deleted Messages"))),
        )
    }

    /** `[Gmail]/Trash` and `INBOX.Trash` are both called Trash. */
    @Test
    fun `a nested trash is still trash`() {
        assertEquals("[Gmail]/Trash", TrashFolder.pick(listOf(folder("[Gmail]/Trash"))))
        assertEquals("INBOX.Trash", TrashFolder.pick(listOf(folder("INBOX.Trash"))))
    }

    /**
     * **Null means do not delete.** Guessing a name and having the
     * server create it puts the message somewhere no other client the
     * person uses will ever look.
     */
    @Test
    fun `no trash folder means no deleting`() {
        assertNull(TrashFolder.pick(listOf(folder("INBOX"), folder("Archive"))))
        assertNull(TrashFolder.pick(emptyList()))
    }

    /** A folder whose name merely contains the word is not it. */
    @Test
    fun `a folder that only mentions trash is not trash`() {
        assertNull(TrashFolder.pick(listOf(folder("Trashy ideas"), folder("Not trash"))))
    }
}
