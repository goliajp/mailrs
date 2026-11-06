package jp.golia.mailrs.wire

import jp.golia.mailrs.R
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
import androidx.work.ExistingWorkPolicy
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import jp.golia.mailrs.widget.WidgetState
import jp.golia.mailrs.widget.refreshInboxWidgets
import java.util.concurrent.TimeUnit

/**
 * "You have mail": a look at the mailbox, run periodically and whenever a
 * UnifiedPush ping arrives.
 *
 * With a UnifiedPush distributor on the phone the server pings it as mail
 * arrives ([PushPingService]) and this runs at once. Without one, the
 * periodic run is all there is: fifteen minutes is `WorkManager`'s floor
 * for periodic work and the system will stretch it further on a phone
 * that is asleep. The periodic run stays on with a distributor too, for
 * the times its server is down.
 *
 * It asks for a **count**, not a list: one small request per check, and
 * the inbox only when something arrived.
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

        val check = NewMailCheck(
            unseen = { (client.unseenCount() as? MailrsClient.Outcome.Ok)?.value },
            // who it is from and what it is about: one extra request, and
            // only when something actually arrived
            inbox = { (client.conversations(MailList.Inbox) as? MailrsClient.Outcome.Ok)?.value },
        )
        // The network is the usual reason the count is missing. Retry
        // rather than success, so a phone that was in a tunnel is checked
        // again soon instead of waiting out the full period.
        val found = check.run(prefs.lastUnseen) ?: return Result.retry()
        prefs.lastUnseen = found.count

        // The home-screen widget draws what was last fetched and never
        // fetches itself, and the list is already in hand — so it is
        // written from this trip rather than from a second one.
        if (found.inbox != null) {
            WidgetState.write(applicationContext, signedIn = true, conversations = found.inbox)
            refreshInboxWidgets(applicationContext)
        }
        val notice = found.notice ?: return Result.success()
        notify(
            context = applicationContext,
            title = notice.title,
            text = notice.text,
            summary = notice.summary,
            threadId = notice.threadId,
            channelId = notice.channelId,
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
            val manager = ContextCompat.getSystemService(context, NotificationManager::class.java)
                ?: return
            manager.createNotificationChannel(
                NotificationChannel(
                    CHANNEL_ID,
                    Texts.get(R.string.settings_new_mail),
                    // Default, not high: ordinary mail is worth a
                    // glance, not an interruption that takes over the
                    // screen. Somebody who wants more can change it —
                    // the channel is theirs, and that is the whole
                    // point of channels.
                    NotificationManager.IMPORTANCE_DEFAULT,
                ).apply { description = Texts.get(R.string.notify_channel_new_desc) },
            )
            manager.createNotificationChannel(
                NotificationChannel(
                    NewMailRule.IMPORTANT_CHANNEL,
                    Texts.get(R.string.notify_channel_important),
                    // High, so it arrives as a heads-up. The separation
                    // is the point: with one channel, silencing the
                    // ordinary silences this too, and that decision
                    // belongs to the reader rather than to this app.
                    NotificationManager.IMPORTANCE_HIGH,
                ).apply { description = Texts.get(R.string.notify_channel_important_desc) },
            )
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
            channelId: String = CHANNEL_ID,
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
            val note = NotificationCompat.Builder(context, channelId)
                .setSmallIcon(android.R.drawable.stat_notify_chat)
                .setContentTitle(title)
                .setContentText(text)
                // Expanded, the whole subject rather than one clipped
                // line. A subject is the only thing that says whether
                // this is worth opening, and mail subjects are long.
                .setStyle(NotificationCompat.BigTextStyle().bigText(text))
                .apply { if (summary != null) setSubText(summary) }
                .setContentIntent(open)
                .setAutoCancel(true)
                .apply {
                    // Filing from the shade, which is what a lock screen
                    // is for: most arriving mail is read once and put
                    // away, and making that two taps instead of five is
                    // the point of the action.
                    if (threadId != null) {
                        // Reply first: it is the action a thumb reaches
                        // for, and on a watch or in a car it is the
                        // only one that means anything.
                        addAction(ReplyFromNotification.action(context, threadId))
                        addAction(
                            NotificationCompat.Action.Builder(
                                android.R.drawable.ic_menu_delete,
                                Texts.get(R.string.action_archive),
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

        /**
         * Look now: a ping said mail arrived.
         *
         * `APPEND_OR_REPLACE`: a ping that lands while a check is running
         * may be for mail that check counted before it arrived, so it
         * queues one more check behind it rather than being dropped.
         */
        fun checkNow(context: Context) {
            WorkManager.getInstance(context).enqueueUniqueWork(
                PING_WORK_NAME,
                ExistingWorkPolicy.APPEND_OR_REPLACE,
                OneTimeWorkRequestBuilder<NewMailWorker>()
                    .setConstraints(
                        Constraints.Builder()
                            .setRequiredNetworkType(NetworkType.CONNECTED)
                            .build(),
                    )
                    .build(),
            )
        }

        private const val PING_WORK_NAME = "new-mail-ping"
    }
}
