package jp.golia.mailrs.wire

import android.content.Context
import android.content.Intent
import androidx.core.app.Person
import androidx.core.content.pm.ShortcutInfoCompat
import androidx.core.content.pm.ShortcutManagerCompat
import androidx.core.graphics.drawable.IconCompat

/**
 * The people at the top of the system share sheet.
 *
 * Sharing a photo *to Alice* in one step is the difference between a
 * mail client the phone knows about and one it merely lists. Android
 * builds that row from dynamic shortcuts marked as share targets, so
 * this publishes one per person written to, most recent first.
 *
 * **Published as somebody is written to, never guessed.** A share sheet
 * offering people who were only ever received from would put strangers
 * one tap from a photo — and the addresses this app holds include every
 * mailing list and every no-reply.
 *
 * The system keeps at most a handful (`getMaxShortcutCountPerActivity`,
 * commonly four or five) and orders them by rank, so the list is capped
 * and re-ranked rather than appended to.
 */
object RecentRecipients {

    private const val CATEGORY = "jp.golia.mailrs.category.SHARE_TARGET"
    private const val KEEP = 4

    /** Remember that a message went to these addresses. */
    fun remember(context: Context, addresses: List<String>) {
        if (addresses.isEmpty()) return
        val prefs = context.getSharedPreferences("mailrs.recents", Context.MODE_PRIVATE)
        val existing = prefs.getString("addresses", "").orEmpty()
            .split('\n')
            .filter { it.isNotBlank() }
        // Most recent first, each address once. A person written to
        // twice should move up rather than appear twice.
        val merged = (addresses.map { it.trim() }.filter { it.isNotEmpty() } + existing)
            .distinct()
            .take(KEEP)
        prefs.edit().putString("addresses", merged.joinToString("\n")).apply()
        publish(context, merged)
    }

    /** Take them all down — signing out takes the address book with it. */
    fun clear(context: Context) {
        context.getSharedPreferences("mailrs.recents", Context.MODE_PRIVATE).edit().clear().apply()
        ShortcutManagerCompat.removeAllDynamicShortcuts(context)
    }

    private fun publish(context: Context, addresses: List<String>) {
        val shortcuts = addresses.mapIndexed { rank, address ->
            val name = SenderIdentity.readableName(address)
            ShortcutInfoCompat.Builder(context, "recipient:$address")
                .setShortLabel(name)
                .setLongLabel(address)
                .setRank(rank)
                .setIcon(IconCompat.createWithResource(context, android.R.drawable.sym_action_email))
                .setCategories(setOf(CATEGORY))
                .setLongLived(true)
                .setPerson(Person.Builder().setName(name).setKey(address).build())
                .setIntent(
                    Intent(Intent.ACTION_VIEW)
                        .setPackage(context.packageName)
                        .setData(android.net.Uri.parse("mailto:$address")),
                )
                .build()
        }
        runCatching { ShortcutManagerCompat.setDynamicShortcuts(context, shortcuts) }
            .onFailure { android.util.Log.w("mailrs", "share targets refused", it) }
    }
}
