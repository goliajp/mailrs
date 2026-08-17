package jp.golia.mailrs.wire

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import java.util.concurrent.TimeUnit

/**
 * "You have mail", without push.
 *
 * The server speaks APNs and an FCM sender needs a Firebase project
 * that does not exist yet, so instant delivery is not available to this
 * app. A mail client that never mentions arriving mail is the
 * alternative, and it is a worse one — a periodic check is what Android
 * offers instead, and it is what this does.
 *
 * **Honest about what it is.** Fifteen minutes is `WorkManager`'s floor
 * for periodic work and the system will stretch it further on a phone
 * that is asleep. This is not push and does not pretend to be; it is
 * the difference between finding out in a quarter of an hour and
 * finding out next time you open the app.
 *
 * It asks for a **count**, not a list: one small request per check, and
 * nothing of the mailbox is downloaded in the background.
 */
class NewMailWorker(
    context: Context,
    params: WorkerParameters,
) : CoroutineWorker(context, params) {

    override suspend fun doWork(): Result {
        val prefs = Prefs(applicationContext)
        if (!prefs.notifyNewMail) return Result.success()

        val client = MailrsClient(TokenStore(applicationContext))
        // Signed out. Not a failure, and not worth retrying — there is
        // nothing to check until somebody signs in again.
        if (client.session == null) return Result.success()

        val count = when (val r = client.unseenCount()) {
            is MailrsClient.Outcome.Ok -> r.value
            // The network is the usual reason. Retry rather than
            // success, so a phone that was in a tunnel is checked again
            // soon instead of waiting out the full period.
            is MailrsClient.Outcome.Err -> return Result.retry()
        }

        val arrived = NewMailRule.arrived(prefs.lastUnseen, count)
        prefs.lastUnseen = count
        if (arrived == null) return Result.success()

        // **Who it is from and what it is about**, which is the whole
        // difference between a notification worth reading on a lock
        // screen and one that says only that something happened. One
        // extra request, and only when something actually arrived.
        val newest = (client.conversations(MailList.Inbox) as? MailrsClient.Outcome.Ok)
            ?.value
            ?.filter { it.unreadCount > 0 }
            ?.maxByOrNull { it.lastDate }
        notify(
            context = applicationContext,
            title = newest?.let { SenderIdentity.readableName(it.participants.firstOrNull().orEmpty()) }
                ?: "Mailrs",
            text = newest?.subject?.ifBlank { "(no subject)" } ?: NewMailRule.text(arrived),
            summary = if (arrived > 1) NewMailRule.text(arrived) else null,
            threadId = newest?.threadId,
        )
        return Result.success()
    }

    companion object {
        const val CHANNEL_ID = "new-mail"
        const val EXTRA_THREAD_ID = "mailrs_open_thread"
        const val NOTIFICATION_ID = 1
        private const val WORK_NAME = "new-mail-check"

        /**
         * The channel has to exist before anything is posted to it, and
         * creating it twice is a no-op — so this is called from both the
         * app's start and the worker rather than tracked.
         */
        fun ensureChannel(context: Context) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "New mail",
                // Default, not high: mail is worth a glance, not an
                // interruption that takes over the screen. Somebody who
                // wants more can change it — the channel is theirs, and
                // that is the whole point of channels.
                NotificationManager.IMPORTANCE_DEFAULT,
            ).apply { description = "When mail arrives while the app is closed" }
            ContextCompat.getSystemService(context, NotificationManager::class.java)
                ?.createNotificationChannel(channel)
        }

        /**
         * @param summary said only when more than one arrived — with a
         *   single message the sender and subject are the whole story,
         *   and "1 new message" underneath them is a line that adds
         *   nothing.
         * @param threadId what tapping opens. Null falls back to the
         *   app, which is what a notification with no particular
         *   message to show should do.
         */
        fun notify(
            context: Context,
            title: String,
            text: String,
            summary: String? = null,
            threadId: String? = null,
        ) {
            ensureChannel(context)
            if (ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) !=
                PackageManager.PERMISSION_GRANTED
            ) {
                // Refused, or never asked. Posting anyway is a silent
                // no-op on the platform; returning here says so.
                return
            }
            val open = PendingIntent.getActivity(
                context,
                0,
                context.packageManager.getLaunchIntentForPackage(context.packageName)
                    ?.addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP)
                    ?.putExtra(EXTRA_THREAD_ID, threadId),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
            val note = NotificationCompat.Builder(context, CHANNEL_ID)
                .setSmallIcon(android.R.drawable.stat_notify_chat)
                .setContentTitle(title)
                .setContentText(text)
                .apply { if (summary != null) setSubText(summary) }
                .setContentIntent(open)
                .setAutoCancel(true)
                .apply {
                    // Filing from the shade, which is what a lock screen
                    // is for: most arriving mail is read once and put
                    // away, and making that two taps instead of five is
                    // the point of the action.
                    if (threadId != null) {
                        addAction(
                            NotificationCompat.Action.Builder(
                                android.R.drawable.ic_menu_delete,
                                "Archive",
                                ArchiveFromNotification.intent(context, threadId),
                            ).build(),
                        )
                    }
                }
                .build()
            NotificationManagerCompat.from(context).notify(NOTIFICATION_ID, note)
        }

        /**
         * Start checking, or stop.
         *
         * `KEEP` rather than `UPDATE`: the app calls this on every
         * launch, and replacing the request each time would reset the
         * period and mean a phone opened often is never checked at all.
         */
        fun schedule(context: Context, enabled: Boolean) {
            val work = WorkManager.getInstance(context)
            if (!enabled) {
                work.cancelUniqueWork(WORK_NAME)
                return
            }
            work.enqueueUniquePeriodicWork(
                WORK_NAME,
                ExistingPeriodicWorkPolicy.KEEP,
                PeriodicWorkRequestBuilder<NewMailWorker>(15, TimeUnit.MINUTES)
                    .setConstraints(
                        Constraints.Builder()
                            .setRequiredNetworkType(NetworkType.CONNECTED)
                            .build(),
                    )
                    .build(),
            )
        }
    }
}
