package jp.golia.mailrs

import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.RemoteImages
import jp.golia.mailrs.wire.SettingsWire
import jp.golia.mailrs.wire.Texts
import jp.golia.mailrs.wire.appPasswords
import jp.golia.mailrs.wire.createMask
import jp.golia.mailrs.wire.endOtherSessions
import jp.golia.mailrs.wire.endSession
import jp.golia.mailrs.wire.fileSender
import jp.golia.mailrs.wire.masks
import jp.golia.mailrs.wire.notifyOverrides
import jp.golia.mailrs.wire.notifyPrefs
import jp.golia.mailrs.wire.notifySender
import jp.golia.mailrs.wire.notifyThread
import jp.golia.mailrs.wire.revokeAppPassword
import jp.golia.mailrs.wire.saveNotifyPrefs
import jp.golia.mailrs.wire.saveVacation
import jp.golia.mailrs.wire.signedInSessions
import jp.golia.mailrs.wire.unifiedPushState
import jp.golia.mailrs.wire.unregisterUnifiedPush
import jp.golia.mailrs.wire.vacation
import kotlinx.coroutines.flow.update

/** A settings page of its own, opened from a row in Settings. */
enum class SettingsPage { Notifications, Push, Sessions, AppPasswords, Vacation, Masked }

fun MailViewModel.openSettingsPage(page: SettingsPage) {
    _state.update { it.copy(settingsPage = page, error = null) }
}

fun MailViewModel.closeSettingsPage() {
    _state.update { it.copy(settingsPage = null) }
}

var MailViewModel.remoteImages: RemoteImages
    get() = prefs.remoteImages
    set(value) {
        prefs.remoteImages = value
    }

/**
 * The value on success; on failure the snackbar says `failure` and the
 * answer is null, so a page never shows what the server did not give.
 */
private suspend fun <T> MailViewModel.settings(failure: Int, call: suspend () -> MailrsClient.Outcome<T>): T? =
    when (val r = call()) {
        is MailrsClient.Outcome.Ok -> r.value
        is MailrsClient.Outcome.Err -> {
            _state.update { it.copy(error = Texts.get(failure)) }
            null
        }
    }

suspend fun MailViewModel.loadNotifyPrefs() = settings(R.string.settings_notify_load_failed) { client.notifyPrefs() }

suspend fun MailViewModel.saveNotifyPrefs(p: SettingsWire.NotifyPrefs) =
    settings(R.string.settings_notify_save_failed) { client.saveNotifyPrefs(p) } != null

suspend fun MailViewModel.loadNotifyOverrides() = settings(R.string.settings_notify_load_failed) { client.notifyOverrides() }

suspend fun MailViewModel.notifySender(address: String, value: String?) =
    settings(R.string.settings_notify_save_failed) { client.notifySender(address, value) } != null

suspend fun MailViewModel.notifyThread(threadId: String, value: String?) =
    settings(R.string.settings_notify_save_failed) { client.notifyThread(threadId, value) } != null

suspend fun MailViewModel.loadSessions() = settings(R.string.settings_sessions_load_failed) { client.signedInSessions() }

suspend fun MailViewModel.endSession(id: String) =
    settings(R.string.settings_session_end_failed) { client.endSession(id) } != null

suspend fun MailViewModel.endOtherSessions() = settings(R.string.settings_session_end_failed) { client.endOtherSessions() } != null

suspend fun MailViewModel.loadAppPasswords() = settings(R.string.settings_app_passwords_load_failed) { client.appPasswords() }

suspend fun MailViewModel.revokeAppPassword(id: String) =
    settings(R.string.settings_app_password_revoke_failed) { client.revokeAppPassword(id) } != null

suspend fun MailViewModel.loadVacation() = settings(R.string.settings_vacation_load_failed) { client.vacation() }

suspend fun MailViewModel.saveVacation(v: SettingsWire.Vacation) = settings(R.string.settings_vacation_save_failed) { client.saveVacation(v) }

suspend fun MailViewModel.loadMasks() = settings(R.string.settings_masks_load_failed) { client.masks() }

suspend fun MailViewModel.createMask(forDomain: String) = settings(R.string.settings_mask_create_failed) { client.createMask(forDomain, "") }

/** Files the sender's mail now and from now on; the snackbar says how many moved. */
suspend fun MailViewModel.fileSender(sender: String, bucket: SettingsWire.Bucket) {
    val moved = settings(R.string.settings_file_sender_failed) { client.fileSender(sender, bucket) } ?: return
    _state.update { it.copy(error = Texts.get(R.string.settings_file_sender_done, moved.moved)) }
}

suspend fun MailViewModel.loadUnifiedPush() = settings(R.string.settings_push_load_failed) { client.unifiedPushState() }

suspend fun MailViewModel.removePushDevice(id: String) =
    settings(R.string.settings_push_remove_failed) { client.unregisterUnifiedPush(id) } != null
