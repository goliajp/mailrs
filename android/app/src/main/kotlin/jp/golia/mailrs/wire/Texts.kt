package jp.golia.mailrs.wire

import android.content.Context
import androidx.annotation.StringRes

/**
 * The app's words, for code that has no Compose scope to ask.
 *
 * Composables read their text with `stringResource`; a view model, a
 * worker or the client building an error message has no composition,
 * so it asks here. Both read the same `res/values*` tables, so a word
 * is translated once whoever shows it.
 *
 * The lookup is set when the process starts ([jp.golia.mailrs.MailrsApp])
 * and by JVM unit tests, which have no Android resources and install one
 * that reads the same XML.
 */
object Texts {
    fun interface Lookup {
        fun text(@StringRes id: Int, args: Array<out Any?>): String
    }

    @Volatile
    private var lookup: Lookup? = null

    fun init(context: Context) {
        val resources = context.applicationContext.resources
        lookup = Lookup { id, args -> resources.getString(id, *args) }
    }

    fun install(l: Lookup) {
        lookup = l
    }

    /** Set [l] and return what was set before, so a test can put it back. */
    internal fun swap(l: Lookup?): Lookup? {
        val before = lookup
        lookup = l
        return before
    }

    fun get(@StringRes id: Int, vararg args: Any?): String {
        val l = checkNotNull(lookup) { "Texts used before the process set it up" }
        return l.text(id, args)
    }
}
