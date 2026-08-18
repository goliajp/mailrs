package jp.golia.mailrs.wire

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.app.RemoteInput
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

/**
 * Answering from the notification shade.
 *
 * The signature Android capability a mail client can offer and a
 * desktop one cannot: an answer typed on the lock screen, sent without
 * the app ever coming to the front. Two taps instead of unlock, open,
 * find, reply, type, send.
 *
 * The reply is built from the **thread**, not from the notification: a
 * notification carries a name and a subject, and a properly threaded
 * reply needs an address, the id it answers, and the subject the thread
 * already has. That costs one request before the send, which is the
 * price of the answer landing in the conversation rather than beside
 * it.
 */
class ReplyFromNotification : BroadcastReceiver() {

    override fun onReceive(context: Context, intent: Intent) {
        val threadId = intent.getStringExtra(NewMailWorker.EXTRA_THREAD_ID) ?: return
        val typed = RemoteInput.getResultsFromIntent(intent)
            ?.getCharSequence(KEY_REPLY)
            ?.toString()
            ?.trim()
            .orEmpty()
        if (typed.isEmpty()) return

        val pending = goAsync()
        CoroutineScope(Dispatchers.IO).launch {
            try {
                send(context, threadId, typed)
            } finally {
                pending.finish()
            }
        }
    }

    private suspend fun send(context: Context, threadId: String, typed: String) {
        val client = MailrsClient(TokenStore(context))
        val session = client.session ?: return
        val messages = (client.thread(threadId) as? MailrsClient.Outcome.Ok)?.value ?: return
        val reply = NotificationReply.of(messages, session.address, typed) ?: return
        val signature = (client.signatures() as? MailrsClient.Outcome.Ok)
            ?.value
            ?.let { MailSignature.preferred(it) }
            .orEmpty()
        val outcome = client.send(
            to = reply.to,
            subject = reply.subject,
            body = MailSignature.append(reply.body, signature),
            inReplyTo = reply.inReplyTo,
        )
        // The shade is told either way. A reply that vanished silently
        // is the failure this app cannot afford: the person has already
        // put the phone down believing it went.
        NewMailWorker.notify(
            context = context,
            title = when (outcome) {
                is MailrsClient.Outcome.Ok -> "Sent"
                is MailrsClient.Outcome.Err -> "Not sent"
            },
            text = when (outcome) {
                is MailrsClient.Outcome.Ok -> reply.to.joinToString(", ")
                is MailrsClient.Outcome.Err -> outcome.message
            },
        )
    }

    companion object {
        const val KEY_REPLY = "mailrs_reply_text"

        /**
         * The action a notification carries. `SetAllowGeneratedReplies`
         * is on so the system may offer its own suggested answers
         * beside the field, and the semantic action is what tells a
         * watch or a car that this is a reply rather than a button.
         */
        fun action(context: Context, threadId: String): NotificationCompat.Action =
            NotificationCompat.Action.Builder(
                android.R.drawable.ic_menu_send,
                "Reply",
                PendingIntent.getBroadcast(
                    context,
                    threadId.hashCode() + 1,
                    Intent(context, ReplyFromNotification::class.java)
                        .putExtra(NewMailWorker.EXTRA_THREAD_ID, threadId),
                    PendingIntent.FLAG_MUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
                ),
            )
                .addRemoteInput(RemoteInput.Builder(KEY_REPLY).setLabel("Reply").build())
                .setSemanticAction(NotificationCompat.Action.SEMANTIC_ACTION_REPLY)
                .setAllowGeneratedReplies(true)
                .build()

        /** Clear the shade — used when the reply screen takes over. */
        fun dismiss(context: Context) {
            NotificationManagerCompat.from(context).cancel(NewMailWorker.NOTIFICATION_ID)
        }
    }
}
