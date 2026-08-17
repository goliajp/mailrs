package jp.golia.mailrs.wire

import android.content.ContentResolver
import android.net.Uri
import okhttp3.MediaType
import okhttp3.MediaType.Companion.toMediaTypeOrNull
import okhttp3.RequestBody
import okio.BufferedSink
import okio.source

/**
 * A file on the phone, sent without being read into memory first.
 *
 * A photo is a few megabytes and a video is not. The obvious version —
 * `resolver.openInputStream(uri).readBytes().toRequestBody()` — holds
 * the whole file and a copy of it at once, so it fails on exactly the
 * attachments worth sending.
 *
 * The declared type is the provider's, not a guess from the extension:
 * the provider knows, and a `.jpeg` named `.txt` should arrive as what
 * it is.
 */
class ContentUriBody(
    private val resolver: ContentResolver,
    private val uri: Uri,
) : RequestBody() {

    override fun contentType(): MediaType? =
        resolver.getType(uri)?.toMediaTypeOrNull() ?: "application/octet-stream".toMediaTypeOrNull()

    /**
     * -1 when the provider will not say, which makes this a chunked
     * upload rather than a wrong `Content-Length`.
     */
    override fun contentLength(): Long =
        runCatching {
            resolver.openAssetFileDescriptor(uri, "r")?.use { it.length } ?: -1L
        }.getOrDefault(-1L)

    override fun writeTo(sink: BufferedSink) {
        val stream = resolver.openInputStream(uri)
            ?: throw java.io.IOException("could not open $uri")
        stream.use { sink.writeAll(it.source()) }
    }
}
