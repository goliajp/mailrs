package jp.golia.mailrs.wire

import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** The sectioned inbox setting's wire shape, the server's `SectionedInbox` (`families/sectioned_inbox.rs`), and its default. */
class SectionedInboxSettingTest {
    @Test
    fun the_servers_shape_decodes_and_encodes_back() {
        val off = Json.decodeFromString(SectionedInboxSetting.serializer(), """{"enabled":false}""")
        assertEquals(SectionedInboxSetting(enabled = false), off)
        assertEquals("""{"enabled":false}""", settingsWrites.encodeToString(SectionedInboxSetting.serializer(), off))
    }

    @Test
    fun on_until_the_reader_turns_it_off() {
        assertTrue(SectionedInboxSetting.Standard.enabled)
    }
}
