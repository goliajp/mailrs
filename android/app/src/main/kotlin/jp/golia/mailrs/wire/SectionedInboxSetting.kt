package jp.golia.mailrs.wire

import kotlinx.serialization.Serializable

/**
 * Whether the Inbox is shown by section (All, To reply, Waiting, New
 * senders). The reader's setting, kept on the server
 * (`/api/settings/sectioned-inbox`) so the web and both apps agree; the
 * same type as the iPhone's `Wire/SectionedInboxSetting.swift`.
 *
 * Backend: `core-sidestate/families/sectioned_inbox.rs` — `SectionedInbox`.
 */
@Serializable
data class SectionedInboxSetting(val enabled: Boolean) {
    companion object {
        /** On until the reader turns it off, the server's default. */
        val Standard = SectionedInboxSetting(enabled = true)
    }
}

suspend fun MailrsClient.sectionedInboxSetting() =
    one(get("/api/settings/sectioned-inbox"), SectionedInboxSetting.serializer())

suspend fun MailrsClient.saveSectionedInboxSetting(s: SectionedInboxSetting) =
    one(
        call("PUT", "/api/settings/sectioned-inbox", settingsWrites.encodeToString(SectionedInboxSetting.serializer(), s)),
        SectionedInboxSetting.serializer(),
    )
