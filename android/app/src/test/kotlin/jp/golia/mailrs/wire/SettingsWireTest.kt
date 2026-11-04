package jp.golia.mailrs.wire

import jp.golia.mailrs.ui.QuietHours
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The same cases as the iPhone's `SettingsWireTests`. */
class SettingsWireTest {
    @Test
    fun images_follow_the_setting() {
        assertFalse(RemoteImages.Auto.blocked(loaded = false))
        assertTrue(RemoteImages.Click.blocked(loaded = false))
        assertFalse(RemoteImages.Click.blocked(loaded = true))
        assertTrue(RemoteImages.Never.blocked(loaded = true))
        assertTrue(RemoteImages.Click.offersButton(loaded = false))
        assertFalse(RemoteImages.Click.offersButton(loaded = true))
        assertFalse(RemoteImages.Never.offersButton(loaded = false))
        assertFalse(RemoteImages.Auto.offersButton(loaded = false))
    }

    /** `enabled` and `days` have no default on the server, so they are always written. */
    @Test
    fun a_vacation_reply_is_written_whole() {
        val sent = settingsWrites.encodeToString(SettingsWire.Vacation.serializer(), SettingsWire.Vacation(body = "Back"))
        assertTrue(sent, sent.contains("\"enabled\":false"))
        assertTrue(sent, sent.contains("\"days\":7"))
        assertTrue(sent, sent.contains("\"version\":0"))
    }

    /** Quiet hours off sends neither end, the only other window the server takes. */
    @Test
    fun quiet_hours_off_sends_neither_end() {
        val off = SettingsWire.NotifyPrefs(mode = "all", quietStart = QuietHours.start(false), quietEnd = QuietHours.end(false))
        val sent = settingsWrites.encodeToString(SettingsWire.NotifyPrefs.serializer(), off)
        assertFalse(sent, sent.contains("quiet_start") || sent.contains("quiet_end"))
        assertTrue(sent, sent.contains("\"mode\":\"all\""))
        assertEquals(1320, QuietHours.start(true))
        assertEquals(420, QuietHours.end(true))
        assertEquals("22:00", QuietHours.label(1320))
    }

    /** Absent fields are the server's defaults, as `Prefs` reads them. */
    @Test
    fun notify_prefs_read_absent_fields_as_the_server_does() {
        val p = settingsWrites.decodeFromString(SettingsWire.NotifyPrefs.serializer(), "{}")
        assertEquals("default", p.mode)
        assertNull(p.quietStart)
        assertFalse(p.hideContent)
    }

    @Test
    fun a_vacation_with_no_text_cannot_be_turned_on() {
        assertFalse(SettingsWire.Vacation(body = "  ").canTurnOn)
        assertTrue(SettingsWire.Vacation(body = "Back on Monday.").canTurnOn)
    }

    @Test
    fun app_passwords_are_made_on_the_web_security_page() {
        assertEquals("https://mail.golia.jp/settings?tab=security", SettingsWire.appPasswordsPage("https://mail.golia.jp/"))
    }
}
