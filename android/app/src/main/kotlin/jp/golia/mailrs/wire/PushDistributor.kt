package jp.golia.mailrs.wire

import android.content.Context
import android.content.pm.PackageManager
import org.unifiedpush.android.connector.UnifiedPush

/**
 * The reader's choice of UnifiedPush distributor: which are installed,
 * which one carries this app's pushes, and turning it on and off.
 */
object PushDistributor {

    /** The distributor apps on this phone, by package name. */
    fun installed(context: Context): List<String> =
        UnifiedPush.getDistributors(context).filter { it != context.packageName }

    /** The distributor this app asked, once it has answered; null before. */
    fun active(context: Context): String? = UnifiedPush.getAckDistributor(context)

    /** The distributor this app asked, answered or not. */
    fun chosen(context: Context): String? = UnifiedPush.getSavedDistributor(context)

    /** What the phone calls the app, for the settings page; uninstalled since, its package name. */
    fun label(context: Context, pkg: String): String {
        val pm = context.packageManager
        return try {
            pm.getApplicationLabel(pm.getApplicationInfo(pkg, 0)).toString()
        } catch (_: PackageManager.NameNotFoundException) {
            pkg
        }
    }

    /**
     * Ask `pkg` for an endpoint. Its answer arrives at [PushPingService],
     * which hands it to the server. `vapid` is the server's key: some
     * distributors refuse a registration without one.
     */
    fun use(context: Context, pkg: String, vapid: String) {
        PushRegistration(context).failure = null
        UnifiedPush.saveDistributor(context, pkg)
        UnifiedPush.register(context, messageForDistributor = "Mailrs", vapid = vapid)
    }

    /** Stop: the distributor is told, and the server's copy goes. */
    fun stop(context: Context) {
        UnifiedPush.unregister(context)
        PushRegistration(context).lost()
        PushSyncWorker.enqueue(context)
    }

    /**
     * Signing out: the server forgets this phone while the session still
     * works, so the next account to sign in here is not pinged for this
     * one's mail. `leaving` is a client that still holds the session.
     */
    suspend fun signedOut(context: Context, leaving: MailrsClient) {
        val reg = PushRegistration(context)
        val id = reg.snapshot().serverId
        UnifiedPush.unregister(context)
        reg.clear()
        if (id != null) leaving.unregisterUnifiedPush(id)
    }
}
