package jp.golia.mailrs.wire

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationManagerCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

/**
 * Filing a message from the notification shade.
 *
 * Most arriving mail is read once and put away, and a lock screen is
 * where that decision gets made — so archiving from there is two taps
 * instead of unlock, open, find, swipe.
 *
 * A receiver rather than an activity: the app does not need to come to
 * the front to file something, and bringing it there would be the
 * opposite of what the person just asked for.
 */
class ArchiveFromNotification : BroadcastReceiver() {

    override fun onReceive(context: Context, intent: Intent) {
        val threadId = intent.getStringExtra(NewMailWorker.EXTRA_THREAD_ID) ?: return
        // The notification goes at once. Waiting for the request would
        // leave it under a thumb that has already moved on, and the
        // request is the kind that either works or is worth nothing.
        NotificationManagerCompat.from(context).cancel(NewMailWorker.NOTIFICATION_ID)

        val pending = goAsync()
        CoroutineScope(Dispatchers.IO).launch {
            try {
                val client = MailrsClient(TokenStore(context))
                if (client.session != null) {
                    client.batch(MailrsClient.Verb.Archive, listOf(threadId))
                }
            } finally {
                // `goAsync` holds the process alive for this work and
                // must be released, or the system kills it after ten
                // seconds and logs a complaint about the receiver.
                pending.finish()
            }
        }
    }

    companion object {
        fun intent(context: Context, threadId: String): PendingIntent = PendingIntent.getBroadcast(
            context,
            threadId.hashCode(),
            Intent(context, ArchiveFromNotification::class.java)
                .putExtra(NewMailWorker.EXTRA_THREAD_ID, threadId),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
    }
}
