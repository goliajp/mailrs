package jp.golia.mailrs.accounts

import android.content.Context
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The rows that were already on a phone when the table arrived.
 *
 * A device upgrading from a build that kept its mail as one JSON string
 * in preferences would otherwise open to an empty list — which reads as
 * lost mail, not as a schema change, and is the kind of thing nobody
 * finds until it is on somebody's phone.
 */
@RunWith(AndroidJUnit4::class)
class RowMigrationTest {
    private lateinit var context: Context

    private fun row(uid: Long) =
        MailboxRow("a", uid, "INBOX", false, "a@x.jp", "s", uid, "<a-$uid>")

    private fun prefs() = context
        .getSharedPreferences("mailrs.accounts", Context.MODE_PRIVATE)

    private fun writeOldBlob(rows: List<MailboxRow>) {
        prefs().edit().putString(
            "mailbox.rows.v1",
            Json.encodeToString(ListSerializer(MailboxRow.serializer()), rows),
        ).apply()
    }

    @Before
    fun clean() {
        context = InstrumentationRegistry.getInstrumentation().targetContext
        // Emptied rather than deleted: AccountStore holds the shared
        // connection open, and removing the file under it leaves that
        // connection pointing at something that is no longer there.
        MailboxDatabase.shared(context).replaceAll(emptyList())
        prefs().edit().clear().apply()
    }

    @After
    fun tidy() {
        MailboxDatabase.shared(context).replaceAll(emptyList())
        prefs().edit().clear().apply()
    }

    @Test
    fun rowsKeptByTheOldBuildAreStillThere() {
        writeOldBlob(listOf(row(1), row(2)))
        assertEquals(listOf(1L, 2L), AccountStore(context).rows().map { it.uid }.sorted())
    }

    // Removed, so that a person who deletes a message and then installs
    // an older build and this one again does not get it back.
    @Test
    fun theOldKeyIsGoneAfterwards() {
        writeOldBlob(listOf(row(1)))
        AccountStore(context).rows()
        assertNull(prefs().getString("mailbox.rows.v1", null))
    }

    @Test
    fun aDeleteAfterTheMoveStaysDeleted() {
        writeOldBlob(listOf(row(1), row(2)))
        val store = AccountStore(context)
        store.deleteRow(row(1))
        assertEquals(listOf(2L), store.rows().map { it.uid })
    }

    // A blob that cannot be read is not a reason to refuse to start;
    // the rows are a cache of what a server has, and the next pass
    // fetches them again.
    @Test
    fun aCorruptBlobCostsTheCacheAndNotTheApp() {
        prefs().edit().putString("mailbox.rows.v1", "{not json").apply()
        assertEquals(emptyList<MailboxRow>(), AccountStore(context).rows())
        assertNull(prefs().getString("mailbox.rows.v1", null))
    }

    @Test
    fun nothingToCarryIsNotAnError() {
        assertEquals(emptyList<MailboxRow>(), AccountStore(context).rows())
    }
}
