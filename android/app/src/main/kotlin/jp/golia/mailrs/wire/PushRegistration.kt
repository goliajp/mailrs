package jp.golia.mailrs.wire

import android.content.Context
import android.content.SharedPreferences

/**
 * What this phone knows about its UnifiedPush registration: the endpoint
 * and keys its distributor gave, and what the server was last told.
 *
 * The two halves are kept apart because they change apart. The distributor
 * hands over a new endpoint whenever it likes, the app may be offline when
 * it does, and [PushSync] makes the server's half catch up.
 */
class PushRegistration(context: Context) {

    private val store = context.getSharedPreferences("mailrs.unifiedpush", Context.MODE_PRIVATE)

    fun snapshot() = PushSync.Snapshot(
        endpoint = store.getString(ENDPOINT, null),
        p256dh = store.getString(P256DH, null),
        auth = store.getString(AUTH, null),
        serverId = store.getString(SERVER_ID, null),
        serverEndpoint = store.getString(SERVER_ENDPOINT, null),
        serverSent = store.getString(SERVER_SENT, null),
    )

    /** The distributor gave an endpoint. */
    fun gave(endpoint: String, p256dh: String, auth: String) {
        store.edit()
            .putString(ENDPOINT, endpoint)
            .putString(P256DH, p256dh)
            .putString(AUTH, auth)
            .remove(FAILURE)
            .apply()
    }

    /** The distributor no longer carries this app's pushes. */
    fun lost() {
        store.edit().remove(ENDPOINT).remove(P256DH).remove(AUTH).apply()
    }

    fun registered(id: String, endpoint: String, sent: String) {
        store.edit()
            .putString(SERVER_ID, id)
            .putString(SERVER_ENDPOINT, endpoint)
            .putString(SERVER_SENT, sent)
            .remove(FAILURE)
            .apply()
    }

    fun dropped() {
        store.edit().remove(SERVER_ID).remove(SERVER_ENDPOINT).remove(SERVER_SENT).apply()
    }

    /**
     * When the server was last found to have dropped this phone and was told
     * again, in epoch milliseconds; null if it never was. Only shown on the
     * settings page, as a trace.
     */
    val reregisteredAt: Long?
        get() = store.getLong(REREGISTERED_AT, 0).takeIf { it > 0 }

    fun reregistered(at: Long) {
        store.edit().putLong(REREGISTERED_AT, at).apply()
    }

    /**
     * The reader just turned pushes on here, and the server has not taken
     * the registration yet. Only such a registration may undo a removal
     * from another device; the ones the app makes on its own may not.
     */
    var turnedOn: Boolean
        get() = store.getBoolean(TURNED_ON, false)
        set(value) = store.edit().putBoolean(TURNED_ON, value).apply()

    /** A person removed this phone from another device, and pushes here were turned off for it. */
    var removedElsewhere: Boolean
        get() = store.getBoolean(REMOVED_ELSEWHERE, false)
        set(value) = store.edit().putBoolean(REMOVED_ELSEWHERE, value).apply()

    /** Why the last step failed, for the settings page; null once one works. */
    var failure: String?
        get() = store.getString(FAILURE, null)
        set(value) = store.edit().putString(FAILURE, value).apply()

    fun clear() {
        store.edit().clear().apply()
    }

    /**
     * Call `onChange` whenever either half changes. The store holds the
     * listener weakly: keep the returned one until [unlisten].
     */
    fun listen(onChange: () -> Unit): SharedPreferences.OnSharedPreferenceChangeListener {
        val l = SharedPreferences.OnSharedPreferenceChangeListener { _, _ -> onChange() }
        store.registerOnSharedPreferenceChangeListener(l)
        return l
    }

    fun unlisten(l: SharedPreferences.OnSharedPreferenceChangeListener) {
        store.unregisterOnSharedPreferenceChangeListener(l)
    }

    private companion object {
        const val ENDPOINT = "endpoint"
        const val P256DH = "p256dh"
        const val AUTH = "auth"
        const val SERVER_ID = "server_id"
        const val SERVER_ENDPOINT = "server_endpoint"
        const val SERVER_SENT = "server_sent"
        const val FAILURE = "failure"
        const val REREGISTERED_AT = "reregistered_at"
        const val TURNED_ON = "turned_on"
        const val REMOVED_ELSEWHERE = "removed_elsewhere"
    }
}

/** What the server has to be told so it holds what the distributor gave. */
object PushSync {

    data class Snapshot(
        val endpoint: String?,
        val p256dh: String?,
        val auth: String?,
        val serverId: String?,
        val serverEndpoint: String?,
        val serverSent: String?,
    )

    sealed interface Step {
        data class Drop(val id: String) : Step
        data class Register(val endpoint: String, val p256dh: String, val auth: String) : Step

        /** A person removed this phone from another device: pushes here go off. */
        data object TurnOff : Step
    }

    /** The endpoint and keys as one value, so a change to either is seen. */
    fun sent(endpoint: String, p256dh: String, auth: String) = "$endpoint $p256dh $auth"

    /**
     * Nothing given: the server's copy goes. Something given that the server
     * does not hold: it is registered, after the server's copy of an older
     * endpoint goes, since an endpoint the distributor replaced still
     * reaches nobody.
     */
    fun steps(s: Snapshot): List<Step> {
        val endpoint = s.endpoint
        val p256dh = s.p256dh
        val auth = s.auth
        if (endpoint == null || p256dh == null || auth == null) {
            return listOfNotNull(s.serverId?.let { Step.Drop(it) })
        }
        if (s.serverId != null && s.serverSent == sent(endpoint, p256dh, auth)) return emptyList()
        val stale = s.serverId?.takeIf { s.serverEndpoint != endpoint }?.let { Step.Drop(it) }
        return listOfNotNull(stale, Step.Register(endpoint, p256dh, auth))
    }

    /** On, and the server was told what the distributor gave: nothing for [steps] to do. */
    fun registered(s: Snapshot) = s.serverId != null && steps(s).isEmpty()

    /**
     * The server may forget this phone without saying so (another device
     * took its place under the per-account cap). Read against what the
     * server lists: registered here and missing there is registered again;
     * anything else, a failed read (`listed` null) included, needs nothing.
     * Missing because a person removed it from another device (`removed`)
     * is not registered again: pushes here are turned off instead.
     */
    fun recheck(s: Snapshot, listed: UnifiedPushWire.State?): List<Step> {
        if (listed == null || !listed.enabled || !registered(s)) return emptyList()
        if (listed.devices.any { it.id == s.serverId }) return emptyList()
        if (s.serverId in listed.removed) return listOf(Step.TurnOff)
        return listOf(Step.Register(s.endpoint!!, s.p256dh!!, s.auth!!))
    }
}
