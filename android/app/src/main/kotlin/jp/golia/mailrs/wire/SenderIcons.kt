package jp.golia.mailrs.wire

import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import android.graphics.BitmapFactory
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/**
 * Brand icons for sender domains, fetched once and kept.
 *
 * A list of forty rows must not be forty requests on every scroll, and
 * the answer for a domain does not change while the app is open — so
 * this is a process-wide cache in front of `/api/icon/{domain}`.
 *
 * **A miss is cached too.** The handler answers 204 for "no icon
 * anywhere", and that is an answer: without keeping it, every scroll
 * past a domain with no icon asks again, and the server walks its
 * cascade again for each one.
 *
 * **Only what Android can decode.** The cascade starts with BIMI, whose
 * logos are SVG, and ends at a `.ico` service; `BitmapFactory` reads
 * neither. Those decode to null and the letter avatar stands, which is
 * indistinguishable on screen from a domain that has no icon — the one
 * place this differs from the web and iOS, and the reason it is
 * written down here rather than left to be noticed.
 */
object SenderIcons {

    /** Present with a null value = asked, and there is none. */
    private val cache = LinkedHashMap<String, ImageBitmap?>()

    /**
     * Asks that have gone out and not come back.
     *
     * Without this, a list where many senders share one domain — a
     * company, a mailing list, the ordinary case — sends one request
     * per row on the first paint: every avatar looks in the cache
     * before any of them has answered, and they all miss. The second
     * caller now waits for the first instead of asking again.
     */
    private val inFlight = HashMap<String, Deferred<ImageBitmap?>>()
    private val lock = Mutex()

    /** What is already known, without asking. Safe to call while composing. */
    fun cached(domain: String): ImageBitmap? = cache[domain]

    fun known(domain: String): Boolean = cache.containsKey(domain)

    suspend fun fetch(client: MailrsClient, domain: String): ImageBitmap? = coroutineScope {
        val waiting = lock.withLock {
            if (cache.containsKey(domain)) return@coroutineScope cache[domain]
            inFlight[domain] ?: async(Dispatchers.IO) { ask(client, domain) }.also { inFlight[domain] = it }
        }
        waiting.await()
    }

    private suspend fun ask(client: MailrsClient, domain: String): ImageBitmap? {
        val bytes = when (val r = client.senderIcon(domain)) {
            is MailrsClient.Outcome.Ok -> r.value
            // Not remembered: a network failure is not an answer about
            // this domain, and caching it would hide the icon for as
            // long as the app is open.
            is MailrsClient.Outcome.Err -> {
                lock.withLock { inFlight.remove(domain) }
                return null
            }
        }
        val image = bytes?.let {
            runCatching { BitmapFactory.decodeByteArray(it, 0, it.size)?.asImageBitmap() }.getOrNull()
        }
        lock.withLock {
            if (cache.size >= MAX) cache.remove(cache.keys.first())
            cache[domain] = image
            inFlight.remove(domain)
        }
        return image
    }

    /** Only the tests need this; the cache is otherwise for the app's life. */
    internal fun clear() {
        cache.clear()
        inFlight.clear()
    }

    private const val MAX = 256
}
