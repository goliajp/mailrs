package jp.golia.mailrs.widget

import android.content.Context
import android.content.Intent
import androidx.glance.action.Action
import androidx.glance.appwidget.action.actionStartActivity

/**
 * Tapping the widget opens the app.
 *
 * The launcher intent rather than a component named here: the app's
 * entry point is one place, and a widget that names an activity
 * directly is a second copy of that decision.
 */
object OpenMailrs {
    fun action(context: Context): Action {
        val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)
            ?: Intent(Intent.ACTION_MAIN)
        return actionStartActivity(launch.addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP))
    }
}
