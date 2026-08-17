package jp.golia.mailrs.wire

import android.content.Context

/**
 * What this device remembers about how the app should look.
 *
 * Plain `SharedPreferences`, not the encrypted store the token lives in:
 * a theme choice is not a secret, and putting it behind the Keystore
 * would mean a broken key locks a person out of light mode.
 *
 * Deliberately small. Anything that belongs to the *account* rather than
 * the device — signature, language, time zone — is the server's and is
 * read from there, or two devices disagree about the same person.
 */
class Prefs(context: Context) {

    private val store = context.getSharedPreferences("mailrs.prefs", Context.MODE_PRIVATE)

    var appearance: Appearance
        get() = Appearance.entries.firstOrNull { it.name == store.getString(KEY_APPEARANCE, null) }
            ?: Appearance.System
        set(value) = store.edit().putString(KEY_APPEARANCE, value.name).apply()

    /**
     * Three states, not a boolean.
     *
     * "Follow the phone" is a real answer and the default one; a
     * two-state switch has to encode it as "whichever the phone was on
     * when they last looked", which stops following it the moment the
     * phone changes.
     */
    enum class Appearance(val label: String) {
        System("System"),
        Light("Light"),
        Dark("Dark"),
    }

    private companion object {
        const val KEY_APPEARANCE = "appearance"
    }
}
