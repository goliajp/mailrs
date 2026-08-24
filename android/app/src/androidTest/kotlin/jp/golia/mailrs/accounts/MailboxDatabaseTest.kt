package jp.golia.mailrs.accounts

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The store, on a device, because SQLite is one.
 *
 * Five of these are the assertions that used to be made against a list
 * in MailboxMergeTest — they are properties of *where the rows are
 * kept*, and where they are kept changed. The rest are things a blob
 * could not have got wrong because it rewrote everything every time,
 * and a table can.
 */
@RunWith(AndroidJUnit4::class)
class MailboxDatabaseTest {
    private lateinit var db: MailboxDatabase

    private fun row(
        account: String,
        uid: Long,
        date: Long? = uid,
        folder: String = "INBOX",
        seen: Boolean = false,
    ) = MailboxRow(account, uid, folder, seen, "a@x.jp", "s", date, "<$account-$uid>")

    // Its own file, not the shared one: a suite that wipes the
    // database the app is using is a suite that can only be run when
    // nothing else is.
    private val name = "mailboxes-test.db"

    @Before
    fun open() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        context.deleteDatabase(name)
        db = MailboxDatabase(context, name)
    }

    @After
    fun close() {
        db.close()
        InstrumentationRegistry.getInstrumentation().targetContext.deleteDatabase(name)
    }

    @Test
    fun aMessageReadTwiceIsOneRow() {
        db.upsert(listOf(row("a", 1), row("a", 2)))
        db.upsert(listOf(row("a", 1)))
        assertEquals(2, db.all().size)
    }

    @Test
    fun theServersFlagsWin() {
        db.upsert(listOf(row("a", 1, seen = false)))
        db.upsert(listOf(row("a", 1, seen = true)))
        assertTrue(db.all().single().seen)
    }

    @Test
    fun newMessagesAreKept() {
        db.upsert(listOf(row("a", 1)))
        db.upsert(listOf(row("a", 2)))
        assertEquals(listOf(1L, 2L), db.all().map { it.uid }.sorted())
    }

    // Every uid held for a renumbered folder is a number that no longer
    // means anything — and nothing else may be caught by that.
    @Test
    fun aRenumberedFolderIsDroppedAndNothingElseIs() {
        db.upsert(
            listOf(
                row("a", 1), row("a", 2),
                row("a", 9, folder = "Archive"),
                row("b", 1),
            ),
        )
        db.deleteFolder("a", "INBOX")
        assertTrue(db.all().any { it.accountId == "a" && it.folder == "Archive" })
        assertTrue(db.all().any { it.accountId == "b" })
        assertEquals(0, db.all().count { it.accountId == "a" && it.folder == "INBOX" })
    }

    @Test
    fun removingAnAccountTakesItsMailWithIt() {
        db.upsert(listOf(row("a", 1), row("b", 2)))
        db.deleteAccount("a")
        assertEquals(listOf("b"), db.all().map { it.accountId })
    }

    // The same uid in two accounts is two messages. A table keyed on
    // uid alone would have shown one of them twice and the other never
    // — which is the reason MailboxRow.id is spelled the way it is.
    @Test
    fun theSameUidInTwoAccountsIsTwoRows() {
        db.upsert(listOf(row("a", 1), row("b", 1), row("a", 1, folder = "Archive")))
        assertEquals(3, db.all().size)
    }

    @Test
    fun deletingAddressesOneRow() {
        db.upsert(listOf(row("a", 1), row("a", 2), row("b", 1)))
        db.delete("a", "INBOX", 1)
        assertEquals(setOf("a/INBOX/2", "b/INBOX/1"), db.all().map { it.id }.toSet())
    }

    @Test
    fun aFlagChangeTouchesNothingElse() {
        db.upsert(listOf(row("a", 1), row("a", 2), row("b", 1)))
        db.setSeen("a", "INBOX", 1, true)
        assertEquals(setOf("a/INBOX/1"), db.all().filter { it.seen }.map { it.id }.toSet())
    }

    // A row's date may be absent — a header a server would not give up
    // — and null is not zero: it sorts last rather than to 1970.
    @Test
    fun aRowWithNoDateSurvivesARoundTrip() {
        db.upsert(listOf(row("a", 1, date = null)))
        assertEquals(null, db.all().single().date)
    }

    // The cap is what the audit was about: the list used to be trimmed
    // by loading every row of every account.
    @Test
    fun theCapIsPerAccount() {
        db.upsert((1L..10L).map { row("a", it) } + (1L..3L).map { row("b", it) })
        db.cap("a", 4)
        assertEquals(4, db.all().count { it.accountId == "a" })
        assertEquals(3, db.all().count { it.accountId == "b" })
    }

    /**
     * The SQL cap and [MailboxApply.capped] drop the same rows.
     *
     * Not "the newest four survive" — that is a claim about an ordering
     * I would be writing twice, and the second copy is the one that
     * drifts. This asks the two implementations the same question and
     * requires the same answer, with a tie on date and an absent date
     * both in the sample because those are where an ordering differs
     * without anybody noticing.
     */
    @Test
    fun theSqlCapAgreesWithTheRuleItWasWrittenFrom() {
        val rows = listOf(
            row("a", 1, date = 100), row("a", 2, date = 100), row("a", 3, date = 300),
            row("a", 10, date = 300), row("a", 4, date = null), row("a", 5, date = 500),
            row("a", 6, date = 50, folder = "Archive"), row("a", 7, date = null),
        )
        for (limit in 0..rows.size) {
            db.replaceAll(rows)
            db.cap("a", limit)
            assertEquals(
                "limit=$limit",
                MailboxApply.capped(rows, limit).map { it.id }.toSet(),
                db.all().map { it.id }.toSet(),
            )
        }
    }

    @Test
    fun replacingKeepsOnlyWhatItWasGiven() {
        db.upsert(listOf(row("a", 1), row("b", 2)))
        db.replaceAll(listOf(row("c", 3)))
        assertEquals(listOf("c/INBOX/3"), db.all().map { it.id })
    }

    @Test
    fun anEmptyWriteIsNotAWipe() {
        db.upsert(listOf(row("a", 1)))
        db.upsert(emptyList())
        db.deleteUids("a", "INBOX", emptyList())
        db.setUidsSeen("a", "INBOX", emptyMap())
        assertEquals(1, db.all().size)
        assertFalse(db.all().single().seen)
    }

    // ---- the windowed read ------------------------------------------
    //
    // The list used to load everything and sort it in memory. These
    // hold the SQL to the rules that read did, rather than to a
    // remembered ordering — two spellings of "newest first" is two
    // orders that agree until they do not.

    private fun sample() = listOf(
        row("a", 1, date = 100), row("a", 2, date = 300),
        row("b", 3, date = 200), row("b", 4, date = null),
        row("a", 5, date = 300, folder = "Archive"),
    )

    @Test
    fun theWindowIsTheSameOrderTheListUses() {
        db.upsert(sample())
        assertEquals(
            MailboxMerge.newestFirst(sample()).map { it.id },
            db.newest(sample().size).map { it.id },
        )
    }

    @Test
    fun theWindowTakesTheNewestNotJustAny() {
        db.upsert(sample())
        assertEquals(
            MailboxMerge.newestFirst(sample()).take(2).map { it.id },
            db.newest(2).map { it.id },
        )
    }

    // Null is no filter; an **empty set** is a filter nothing
    // satisfies. Somebody who unticked every box gets an empty list,
    // not the unfiltered one.
    @Test
    fun anEmptyFilterIsNotNoFilter() {
        db.upsert(sample())
        assertEquals(5, db.newest(50, null).size)
        assertEquals(0, db.newest(50, emptySet()).size)
        assertEquals(3, db.newest(50, setOf("a")).size)
    }

    @Test
    fun theSqlSearchAgreesWithTheRuleItWasWrittenFrom() {
        val rows = listOf(
            row("a", 1, date = 300).copy(sender = "Ada", subject = "Lunch"),
            row("a", 2, date = 200).copy(sender = "Bob", subject = "Lunch tomorrow"),
            row("b", 3, date = 100).copy(sender = "Ada", subject = "Dinner"),
        )
        db.upsert(rows)
        for (query in listOf("lunch", "ada", "ada lunch", "ada dinner", "", "zzz")) {
            assertEquals(
                query,
                MailboxSearch.matches(MailboxMerge.newestFirst(rows), query).map { it.id },
                db.search(MailboxSearch.words(query), 50).map { it.id },
            )
        }
    }

    // **Where the two would have parted.** SQLite's `lower` folds
    // ASCII and nothing else, so a subject with an accent matched in
    // memory and not in SQL — a divergence in exactly the alphabets
    // nobody writes a test with. The folded text is stored instead, by
    // the same function the in-memory search uses.
    @Test
    fun anAccentedSubjectIsFoundTheSameWayBothWays() {
        val rows = listOf(
            row("a", 1, date = 300).copy(sender = "Ämile", subject = "RÉUNION"),
            row("a", 2, date = 200).copy(sender = "山田 太郎", subject = "領収書"),
        )
        db.upsert(rows)
        for (query in listOf("réunion", "ämile", "領収書", "RÉUNION")) {
            assertEquals(
                query,
                MailboxSearch.matches(MailboxMerge.newestFirst(rows), query).map { it.id },
                db.search(MailboxSearch.words(query), 50).map { it.id },
            )
        }
    }

    // A row edited must not keep the text it used to be searchable by.
    @Test
    fun theFoldedTextFollowsAnUpdate() {
        db.upsert(listOf(row("a", 1).copy(subject = "Lunch")))
        db.upsert(listOf(row("a", 1).copy(subject = "Dinner")))
        assertEquals(0, db.search(listOf("lunch"), 50).size)
        assertEquals(1, db.search(listOf("dinner"), 50).size)
    }

    @Test
    fun unreadIsCountedPerAccountOverEverything() {
        db.upsert(
            listOf(
                row("a", 1, seen = false), row("a", 2, seen = true),
                row("a", 3, seen = false), row("b", 4, seen = false),
            ),
        )
        assertEquals(mapOf("a" to 2, "b" to 1), db.unreadPerAccount())
    }

    // An account with nothing unread is **absent**, not zero — the
    // chip reads the map and a 0 would draw an empty badge.
    @Test
    fun anAccountWithNothingUnreadIsAbsent() {
        db.upsert(listOf(row("a", 1, seen = true)))
        assertEquals(emptyMap<String, Int>(), db.unreadPerAccount())
    }

    @Test
    fun foldersAreTheOnesThisDeviceHoldsSomethingOf() {
        db.upsert(
            listOf(
                row("a", 1), row("a", 2, folder = "Archive"),
                row("a", 3, folder = "Archive"), row("b", 4, folder = "Sent"),
            ),
        )
        assertEquals(setOf("INBOX", "Archive"), db.folders("a").toSet())
        assertEquals(listOf("Sent"), db.folders("b"))
    }

    @Test
    fun countIsPerAccount() {
        db.upsert(listOf(row("a", 1), row("a", 2), row("b", 3)))
        assertEquals(2, db.count("a"))
        assertEquals(0, db.count("nobody"))
    }
}
