package jp.golia.mailrs

import android.app.NotificationManager
import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.work.ListenableWorker
import androidx.work.testing.TestListenableWorkerBuilder
import jp.golia.mailrs.wire.NewMailWorker
import jp.golia.mailrs.wire.Prefs
import jp.golia.mailrs.wire.TokenStore
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The periodic check, run for real.
 *
 * Push is not available to this app, so this is how anybody finds out
 * mail arrived without opening it. The rule about *when* to say
 * something is unit-tested; this runs the whole worker — token, HTTP,
 * preference, notification — against the same stub the flow tests use.
 */
@RunWith(AndroidJUnit4::class)
class NewMailWorkerTest : GrantsNotifications() {

    private val context: Context = ApplicationProvider.getApplicationContext()

    @Before
    fun signedInAgainstTheStub() {
        val stub = InstrumentationRegistry.getArguments().getString("mailrsBaseURL")
            ?: "http://127.0.0.1:6039"
        TokenStore(context).write(TokenStore.Session(stub, "test-token"))
        Prefs(context).notifyNewMail = true
        Prefs(context).lastUnseen = null
        NotificationManagerCompatShim.clear(context)
    }

    /**
     * **The first check is silent.** There is no "before", and the
     * unread mailbox is the backlog rather than news — so this asserts
     * nothing was posted, and that the count was recorded.
     */
    @Test
    fun the_first_check_records_and_says_nothing() = runBlocking {
        val result = TestListenableWorkerBuilder<NewMailWorker>(context).build().doWork()
        assertEquals(ListenableWorker.Result.success(), result)
        assertEquals(3, Prefs(context).lastUnseen)
        assertEquals(0, active(context))
    }

    /**
     * The notification names the sender and the subject.
     *
     * "2 new messages" on a lock screen says only that something
     * happened; "Alice Smith — Quarterly report" is the thing worth
     * reading without unlocking. It costs one extra request, and only
     * when something actually arrived.
     */
    @Test
    fun the_notification_says_who_it_is_from_and_what_it_is_about() = runBlocking {
        Prefs(context).lastUnseen = 1
        TestListenableWorkerBuilder<NewMailWorker>(context).build().doWork()
        assertTrue("nothing was posted", waitForNotification())

        val posted = context.getSystemService(NotificationManager::class.java)
            .activeNotifications
            .first { it.id == NewMailWorker.NOTIFICATION_ID }
            .notification
        assertEquals(
            "Alice Smith",
            posted.extras.getCharSequence(android.app.Notification.EXTRA_TITLE)?.toString(),
        )
        assertEquals(
            "Quarterly report and the follow-up notes",
            posted.extras.getCharSequence(android.app.Notification.EXTRA_TEXT)?.toString(),
        )
        // And something to do with it without opening the app.
        assertTrue(
            "there was no Archive action",
            posted.actions.orEmpty().any { it.title.toString() == "Archive" },
        )
    }

    /** A rise since the last check is what gets said out loud. */
    @Test
    fun a_rise_since_the_last_check_notifies() = runBlocking {
        // The stub answers 3. Pretend the last check saw one.
        Prefs(context).lastUnseen = 1
        val result = TestListenableWorkerBuilder<NewMailWorker>(context).build().doWork()
        assertEquals(ListenableWorker.Result.success(), result)
        assertTrue("nothing was posted for two new messages", waitForNotification())
    }

    /** Switched off, it does not even ask the server. */
    @Test
    fun switched_off_it_stays_quiet() = runBlocking {
        Prefs(context).notifyNewMail = false
        Prefs(context).lastUnseen = 1
        TestListenableWorkerBuilder<NewMailWorker>(context).build().doWork()
        assertEquals(0, active(context))
        // Untouched: a check that did not run has nothing to record.
        assertEquals(1, Prefs(context).lastUnseen)
    }

    /**
     * The system takes the notification asynchronously, so reading the
     * active list the instant `doWork` returns is a race — it passed
     * once and failed the next run with nothing changed. Polls instead.
     */
    private fun waitForNotification(): Boolean {
        repeat(50) {
            if (active(context) > 0) return true
            Thread.sleep(100)
        }
        return false
    }

    private fun active(context: Context): Int {
        val nm = context.getSystemService(NotificationManager::class.java)
        return nm.activeNotifications.count { it.id == NewMailWorker.NOTIFICATION_ID }
    }
}

/** Cancelling by hand, so one test cannot see another's notification. */
private object NotificationManagerCompatShim {
    fun clear(context: Context) {
        context.getSystemService(NotificationManager::class.java)
            .cancel(NewMailWorker.NOTIFICATION_ID)
    }
}
