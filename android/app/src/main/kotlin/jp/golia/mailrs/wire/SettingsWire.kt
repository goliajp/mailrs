package jp.golia.mailrs.wire

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * The reader's own settings: push, signed-in sessions, app passwords, the
 * vacation reply, masked addresses and where a sender's mail is filed.
 * Every route is the one the web's settings page calls; the same types as
 * the iPhone's `Wire/SettingsWire.swift`.
 */
object SettingsWire {
    /** Backend: `core-sidestate/families/notify.rs` — `Prefs`. Quiet hours are minutes after local midnight. */
    @Serializable
    data class NotifyPrefs(
        val mode: String = "default",
        @SerialName("quiet_start") val quietStart: Int? = null,
        @SerialName("quiet_end") val quietEnd: Int? = null,
        @SerialName("utc_offset_minutes") val utcOffsetMinutes: Int = 0,
        @SerialName("hide_content") val hideContent: Boolean = false,
    )

    /** Backend: `handlers/notify.rs` — `Overrides`, pairs of key and choice. */
    @Serializable
    data class Overrides(val senders: List<List<String>> = emptyList(), val threads: List<List<String>> = emptyList())

    /** Backend: `handlers/auth/sessions.rs` — `SessionView`. */
    @Serializable
    data class SignedInSession(
        val id: String,
        @SerialName("created_at") val createdAt: Long = 0,
        @SerialName("last_seen") val lastSeen: Long = 0,
        val ip: String = "",
        @SerialName("user_agent") val userAgent: String = "",
        val current: Boolean = false,
    )

    /** Backend: `handlers/app_passwords.rs` — `list`. */
    @Serializable
    data class AppPassword(
        val id: String,
        val name: String,
        @SerialName("created_at") val createdAt: Long = 0,
        @SerialName("last_used_at") val lastUsedAt: Long? = null,
    )

    /** Backend: `core-sidestate/families/vacation.rs` — `Vacation`. Sent back whole, with only `enabled` changed. */
    @Serializable
    data class Vacation(
        val enabled: Boolean = false,
        val subject: String = "",
        val body: String = "",
        val start: Long? = null,
        val end: Long? = null,
        val days: Int = 7,
        val version: Long = 0,
    ) {
        /** Turning it on with no text is refused (`invalid_body`), so the phone does not offer it. */
        val canTurnOn: Boolean get() = body.isNotBlank()
    }

    /** Backend: `core-sidestate/families/masked_email` — `Mask`, camelCase. */
    @Serializable
    data class Mask(
        val id: String,
        val email: String,
        val state: String = "",
        val forDomain: String? = null,
        val description: String? = null,
    )

    enum class Bucket(val wire: String) { Inbox("inbox"), Notifications("notifications"), Promotions("promotions") }

    /** The web settings page where app passwords are made. */
    fun appPasswordsPage(server: String): String = server.trimEnd('/') + "/settings?tab=security"

    @Serializable
    internal data class OverrideBody(val value: String?)

    @Serializable
    internal data class MaskBody(val forDomain: String, val description: String)

    @Serializable
    internal data class ReclassifyBody(val sender: String, val bucket: String)

    @Serializable
    data class Moved(val moved: Int = 0)

    @Serializable
    data class Ended(val ended: Int = 0)
}

// every field written, defaults included: the vacation reply's `enabled` and `days` have no default on the server
internal val settingsWrites = kotlinx.serialization.json.Json { encodeDefaults = true; explicitNulls = false }

private fun seg(s: String) = java.net.URLEncoder.encode(s, Charsets.UTF_8).replace("+", "%20")

suspend fun MailrsClient.notifyPrefs() = one(get("/api/notify"), SettingsWire.NotifyPrefs.serializer())

suspend fun MailrsClient.saveNotifyPrefs(p: SettingsWire.NotifyPrefs) =
    call("PUT", "/api/notify", settingsWrites.encodeToString(SettingsWire.NotifyPrefs.serializer(), p))

suspend fun MailrsClient.notifyOverrides() = one(get("/api/notify/overrides"), SettingsWire.Overrides.serializer())

/** `always`, `never`, or null to go back to the mode's rule. */
suspend fun MailrsClient.notifySender(address: String, value: String?) =
    call("PUT", "/api/notify/senders/" + seg(address), settingsWrites.encodeToString(SettingsWire.OverrideBody.serializer(), SettingsWire.OverrideBody(value)))

/** `follow`, `mute`, or null to go back to the mode's rule. */
suspend fun MailrsClient.notifyThread(threadId: String, value: String?) =
    call("PUT", "/api/notify/threads/" + seg(threadId), settingsWrites.encodeToString(SettingsWire.OverrideBody.serializer(), SettingsWire.OverrideBody(value)))

suspend fun MailrsClient.signedInSessions() = decode(get("/api/auth/sessions"), SettingsWire.SignedInSession.serializer())

suspend fun MailrsClient.endSession(id: String) = call("DELETE", "/api/auth/sessions/" + seg(id))

suspend fun MailrsClient.endOtherSessions() = one(call("POST", "/api/auth/sessions/end-others", "{}"), SettingsWire.Ended.serializer())

suspend fun MailrsClient.appPasswords() = decode(get("/api/auth/app-passwords"), SettingsWire.AppPassword.serializer())

suspend fun MailrsClient.revokeAppPassword(id: String) = call("DELETE", "/api/auth/app-passwords/" + seg(id))

suspend fun MailrsClient.vacation() = one(get("/api/settings/vacation"), SettingsWire.Vacation.serializer())

suspend fun MailrsClient.saveVacation(v: SettingsWire.Vacation) =
    one(call("PUT", "/api/settings/vacation", settingsWrites.encodeToString(SettingsWire.Vacation.serializer(), v)), SettingsWire.Vacation.serializer())

suspend fun MailrsClient.masks() = decode(get("/api/masked-emails"), SettingsWire.Mask.serializer())

suspend fun MailrsClient.createMask(forDomain: String, description: String) =
    one(
        call("POST", "/api/masked-emails", settingsWrites.encodeToString(SettingsWire.MaskBody.serializer(), SettingsWire.MaskBody(forDomain, description))),
        SettingsWire.Mask.serializer(),
    )

/** Files the sender's conversations to `bucket` now and from now on. */
suspend fun MailrsClient.fileSender(sender: String, bucket: SettingsWire.Bucket) =
    one(
        call("POST", "/api/senders/reclassify", settingsWrites.encodeToString(SettingsWire.ReclassifyBody.serializer(), SettingsWire.ReclassifyBody(sender, bucket.wire))),
        SettingsWire.Moved.serializer(),
    )
