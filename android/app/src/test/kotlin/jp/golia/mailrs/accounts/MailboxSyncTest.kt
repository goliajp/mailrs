package jp.golia.mailrs.accounts

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Which folders a pass reads. */
class MailboxSyncTest {
    @Test
    fun `an ordinary folder is read`() {
        assertTrue(MailboxSync.worthReading("INBOX", listOf("\\HasNoChildren"), emptyList()))
        assertTrue(MailboxSync.worthReading("Work/Clients", emptyList(), emptyList()))
    }

    // A node in the tree rather than a mailbox: `SELECT` on it fails,
    // and a pass that tries loses the folders after it.
    @Test
    fun `a folder that cannot be opened is not tried`() {
        assertFalse(MailboxSync.worthReading("[Gmail]", listOf("\\Noselect"), emptyList()))
    }

    // A view holding a copy of everything doubles the mailbox.
    @Test
    fun `a view holding everything is skipped`() {
        assertFalse(MailboxSync.worthReading("[Gmail]/All Mail", listOf("\\All"), emptyList()))
    }

    /**
     * **A decision rather than an omission.** This list is what
     * arrived: a draft has not been sent to anybody, and a copy of
     * everything the person wrote interleaved by date with what they
     * received is what every "all inboxes" view deliberately does not
     * show.
     */
    @Test
    fun `sent and drafts are not part of an inbox`() {
        assertFalse(MailboxSync.worthReading("Sent", listOf("\\Sent"), emptyList()))
        assertFalse(MailboxSync.worthReading("[Gmail]/Drafts", listOf("\\Drafts"), emptyList()))
    }

    /**
     * A folder nobody has labelled is a folder somebody made, and
     * those are where filed mail lives — so an unmarked server is read
     * whole.
     */
    @Test
    fun `an unlabelled folder is read`() {
        assertTrue(MailboxSync.worthReading("Receipts", emptyList(), emptyList()))
        assertTrue(MailboxSync.worthReading("INBOX.Work", emptyList(), emptyList()))
    }

    @Test
    fun `the bin and the spam are left alone`() {
        assertFalse(MailboxSync.worthReading("Trash", listOf("\\Trash"), emptyList()))
        assertFalse(MailboxSync.worthReading("Spam", listOf("\\Junk"), emptyList()))
    }

    // Not every server sets the attributes, so the provider table names
    // them too — and the names are matched without case, because
    // servers disagree about it.
    @Test
    fun `a named folder is skipped even with no attribute`() {
        assertFalse(MailboxSync.worthReading("已删除", emptyList(), listOf("已删除")))
        assertFalse(MailboxSync.worthReading("TRASH", emptyList(), listOf("Trash")))
        assertTrue(MailboxSync.worthReading("Trashy ideas", emptyList(), listOf("Trash")))
    }
}
