package jp.golia.mailrs.widget

import android.content.Context
import android.content.Intent
import androidx.glance.action.Action
import androidx.glance.appwidget.action.actionStartActivity
import jp.golia.mailrs.wire.NewMailWorker

/**
 * Tapping the widget opens the app.
 *
 * The launcher intent rather than a component named here: the app's
 * entry point is one place, and a widget that names an activity
 * directly is a second copy of that decision.
 */
object OpenMailrs {

    /**
     * @param threadId which conversation to open, or null for the app
     *   itself. The same extra a notification carries, so the activity
     *   has one way in rather than two.
     */
    fun action(context: Context, threadId: String? = null): Action =
        actionStartActivity(intent(context, threadId))

    /**
     * The intent behind [action], separately, because a Glance `Action`
     * is opaque once built — a test can read this and cannot read that.
     */
    fun intent(context: Context, threadId: String? = null): Intent =
        (
            context.packageManager.getLaunchIntentForPackage(context.packageName)
                ?: Intent(Intent.ACTION_MAIN)
            )
            .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP)
            .putExtra(NewMailWorker.EXTRA_THREAD_ID, threadId)
}
