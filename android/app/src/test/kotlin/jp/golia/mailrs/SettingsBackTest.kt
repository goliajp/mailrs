package jp.golia.mailrs

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Back, from inside Settings.
 *
 * The defect these pin: Settings' sub-screens are not their own
 * `Screen`, so one back arm closed the whole of Settings from inside
 * them — a person who opened Mail accounts and pressed back landed on
 * the mail list, two screens away from where they were.
 */
class SettingsBackTest {
    @Test
    fun `back from mail accounts returns to settings`() {
        val state = UiState(settingsOpen = true, mailAccountsOpen = true)
        assertEquals(SettingsBack.MailAccounts, settingsBack(state))
    }

    @Test
    fun `back from other mail returns to settings`() {
        val state = UiState(settingsOpen = true, mergedMailOpen = true)
        assertEquals(SettingsBack.MergedMail, settingsBack(state))
    }

    @Test
    fun `back from settings itself leaves settings`() {
        assertEquals(SettingsBack.Settings, settingsBack(UiState(settingsOpen = true)))
    }

    /**
     * Both open at once should not be reachable, but a rule that
     * returns nothing in a state it did not expect is a rule that
     * traps somebody. The inner-most screen wins.
     */
    @Test
    fun `both open closes the inner one first`() {
        val state = UiState(settingsOpen = true, mailAccountsOpen = true, mergedMailOpen = true)
        assertEquals(SettingsBack.MailAccounts, settingsBack(state))
    }
}
