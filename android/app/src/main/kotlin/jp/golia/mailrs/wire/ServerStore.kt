package jp.golia.mailrs.wire

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.serialization.json.Json

/**
 * The saved servers, kept between launches.
 *
 * Plain preferences of their own, not the token's: an address is not a
 * secret, and signing out clears the token's file, which is exactly
 * when this list is needed.
 *
 * @param previous the server of the session this install already had,
 * read once, on the first launch that has no list yet.
 */
class ServerStore(context: Context, previous: () -> String?) {

    private val prefs = context.getSharedPreferences("mailrs.servers", Context.MODE_PRIVATE)

    private val _list = MutableStateFlow(
        read() ?: ServerList.afterUpgrade(previous()).also { write(it) },
    )
    val list: StateFlow<ServerList> = _list.asStateFlow()

    /** Change the list and write it, unless nothing changed. */
    fun change(change: (ServerList) -> ServerList) {
        val next = change(_list.value)
        if (next == _list.value) return
        write(next)
        _list.value = next
    }

    private fun read(): ServerList? {
        val stored = prefs.getString(KEY, null) ?: return null
        return runCatching { Json.decodeFromString(ServerList.serializer(), stored) }.getOrNull()
    }

    private fun write(list: ServerList) {
        prefs.edit().putString(KEY, Json.encodeToString(ServerList.serializer(), list)).apply()
    }

    private companion object {
        const val KEY = "list"
    }
}
