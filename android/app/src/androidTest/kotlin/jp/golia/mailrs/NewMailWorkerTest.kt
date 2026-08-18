package jp.golia.mailrs

import android.app.NotificationManager
import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.work.ListenableWorker
import androidx.work.testing.TestListenableWorkerBuilder
import jp.golia.mailrs.wire.NewMailRule
import jp.golia.mailrs.wire.NewMailWorker
import jp.golia.mailrs.wire.Prefs
import jp.golia.mailrs.wire.ReplyFromNotification
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

    private val appContext: Context = ApplicationProvider.getApplicationContext()

    /**
     * Leave the shade as it was found.
     *
     * This class posts notifications, one of them on the
     * high-importance channel, which arrives as a heads-up window over
     * whatever is in front. Clearing only at the start of each test
     * leaves the last one up for whoever runs next.
     */
    @org.junit.After
    fun theShadeIsLeftEmpty() {
        NotificationManagerCompatShim.clear(appContext)
    }

    @Before
    fun signedInAgainstTheStub() {
        val stub = StubServer.base()
        TokenStore(appContext).write(TokenStore.Session(stub, "test-token", "me@golia.jp"))
        Prefs(appContext).notifyNewMail = true
        Prefs(appContext).lastUnseen = null
        NotificationManagerCompatShim.clear(appContext)
    }

    /**
     * **The first check is silent.** There is no "before", and the
     * unread mailbox is the backlog rather than news — so this asserts
     * nothing was posted, and that the count was recorded.
     */
    @Test
    fun the_first_check_records_and_says_nothing() = runBlocking {
        val result = TestListenableWorkerBuilder<NewMailWorker>(appContext).build().doWork()
        assertEquals(ListenableWorker.Result.success(), result)
        assertEquals(3, Prefs(appContext).lastUnseen)
        assertEquals(0, active(appContext))
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
        Prefs(appContext).lastUnseen = 1
        TestListenableWorkerBuilder<NewMailWorker>(appContext).build().doWork()
        assertTrue("nothing was posted", waitForNotification())

        val posted = appContext.getSystemService(NotificationManager::class.java)
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
        Prefs(appContext).lastUnseen = 1
        val result = TestListenableWorkerBuilder<NewMailWorker>(appContext).build().doWork()
        assertEquals(ListenableWorker.Result.success(), result)
        assertTrue("nothing was posted for two new messages", waitForNotification())
    }

    /** Switched off, it does not even ask the server. */
    @Test
    fun switched_off_it_stays_quiet() = runBlocking {
        Prefs(appContext).notifyNewMail = false
        Prefs(appContext).lastUnseen = 1
        TestListenableWorkerBuilder<NewMailWorker>(appContext).build().doWork()
        assertEquals(0, active(appContext))
        // Untouched: a check that did not run has nothing to record.
        assertEquals(1, Prefs(appContext).lastUnseen)
    }

    /**
     * The system takes the notification asynchronously, so reading the
     * active list the instant `doWork` returns is a race — it passed
     * once and failed the next run with nothing changed. Polls instead.
     */
    private fun waitForNotification(): Boolean {
        repeat(50) {
            if (active(appContext) > 0) return true
            Thread.sleep(100)
        }
        return false
    }

    private fun active(context: Context): Int {
        val nm = appContext.getSystemService(NotificationManager::class.java)
        return nm.activeNotifications.count { it.id == NewMailWorker.NOTIFICATION_ID }
    }

    /**
     * An answer typed in the shade is sent, threaded, to the right
     * person.
     *
     * The whole point of direct reply is that the app never comes to
     * the front, so nothing about this is visible on a screen — the
     * assertion is what the server received. It goes through the
     * receiver the notification's action points at, with the text
     * attached the way the system attaches it.
     */
    @Test
    fun a_reply_typed_in_the_shade_reaches_the_right_person() {
        val ctx = appContext
        val intent = android.content.Intent(ctx, ReplyFromNotification::class.java)
            .putExtra(NewMailWorker.EXTRA_THREAD_ID, "t1")
        androidx.core.app.RemoteInput.addResultsToIntent(
            arrayOf(androidx.core.app.RemoteInput.Builder(ReplyFromNotification.KEY_REPLY).build()),
            intent,
            android.os.Bundle().apply { putCharSequence(ReplyFromNotification.KEY_REPLY, "From the shade.") },
        )
        ctx.sendBroadcast(intent)

        var sent = ""
        repeat(60) {
            sent = readStub("/debug/sent")
            if (sent.contains("From the shade.")) return@repeat
            Thread.sleep(250)
        }
        assertTrue("the shade reply never reached the server: $sent", sent.contains("From the shade."))
        // The newest message that is not mine — uid 2, from the spoofed
        // address — is what a reply answers, not the thread's first.
        assertTrue("the reply did not answer the newest message: $sent", sent.contains("<m2@x>"))
        assertTrue("the reply went to the wrong person: $sent", sent.contains("spoofed@example.com"))
    }

    /**
     * Important mail arrives on the channel a person can keep loud.
     *
     * Two channels only matter if the notification actually goes to
     * the right one — a single-channel app that *names* a second
     * channel gives the reader a switch that does nothing.
     */
    @Test
    fun importance_decides_the_channel() {
        // Each half starts from an empty shade. Without this the first
        // reading is whatever an earlier test left behind, which is a
        // notification with the same id and the wrong channel — the
        // measurement, not the code, and it looked exactly like a
        // defect.
        clearShade()
        NewMailWorker.notify(
            appContext,
            title = "Alice Smith",
            text = "Quarterly report",
            channelId = NewMailRule.channelFor("critical"),
        )
        assertEquals(NewMailRule.IMPORTANT_CHANNEL, postedChannel())

        clearShade()
        NewMailWorker.notify(
            appContext,
            title = "Alice Smith",
            text = "Quarterly report",
            channelId = NewMailRule.channelFor("normal"),
        )
        assertEquals(NewMailWorker.CHANNEL_ID, postedChannel())
    }

    private fun clearShade() {
        val nm = appContext.getSystemService(NotificationManager::class.java)
        nm.cancel(NewMailWorker.NOTIFICATION_ID)
        repeat(40) {
            if (nm.activeNotifications.none { it.id == NewMailWorker.NOTIFICATION_ID }) return
            Thread.sleep(100)
        }
    }

    private fun postedChannel(): String {
        val nm = appContext.getSystemService(NotificationManager::class.java)
        repeat(40) {
            val posted = nm.activeNotifications.firstOrNull { it.id == NewMailWorker.NOTIFICATION_ID }
            if (posted != null) return posted.notification.channelId
            Thread.sleep(100)
        }
        return "nothing was posted"
    }

    /** This class has no compose rule, so it fetches its own. */
    private fun readStub(path: String): String {
        val stub = StubServer.base()
        return java.net.URL(stub + path).openStream().bufferedReader().use { it.readText() }
    }
}

/** Cancelling by hand, so one test cannot see another's notification. */
private object NotificationManagerCompatShim {
    fun clear(context: Context) {
        context.getSystemService(NotificationManager::class.java)
            .cancel(NewMailWorker.NOTIFICATION_ID)
    }
}
