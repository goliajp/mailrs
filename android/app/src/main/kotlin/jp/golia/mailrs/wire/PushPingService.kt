package jp.golia.mailrs.wire

import org.unifiedpush.android.connector.FailedReason
import org.unifiedpush.android.connector.PushService
import org.unifiedpush.android.connector.data.PushEndpoint
import org.unifiedpush.android.connector.data.PushMessage

/**
 * What the reader's UnifiedPush distributor tells this app.
 *
 * A message is only ever "look now": the server sends no content, so the
 * message's bytes are not read, and one that did not decrypt is treated
 * the same. Whoever can push to the endpoint can at most make the app
 * look at its own mailbox.
 */
class PushPingService : PushService() {

    override fun onNewEndpoint(endpoint: PushEndpoint, instance: String) {
        val reg = PushRegistration(this)
        val keys = endpoint.pubKeySet
        if (keys == null) {
            // the server only sends encrypted pushes, and has nothing to
            // encrypt them to
            reg.failure = FailedReason.INTERNAL_ERROR.name
            return
        }
        reg.gave(endpoint.url, keys.pubKey, keys.auth)
        PushSyncWorker.enqueue(this)
    }

    override fun onMessage(message: PushMessage, instance: String) {
        NewMailWorker.checkNow(this)
    }

    override fun onRegistrationFailed(reason: FailedReason, instance: String) {
        PushRegistration(this).failure = reason.name
    }

    override fun onUnregistered(instance: String) {
        PushRegistration(this).lost()
        PushSyncWorker.enqueue(this)
    }
}
