package jp.golia.mailrs.accounts

/**
 * A file chosen to send, before its bytes are read.
 *
 * The URI and the name are kept; **the bytes are not**. A 25 MB file
 * held in memory for as long as somebody is writing a message is 25 MB
 * of a phone's memory doing nothing, and a draft left open overnight is
 * a draft that gets killed for it. They are read when the message is
 * sent, once.
 */
data class PickedFile(
    val uri: String,
    val filename: String,
    val size: Long,
    val mimeType: String,
)

/**
 * What went wrong reading the files back, if anything.
 *
 * **A file that did not come through has to be said.** A provider can
 * refuse to answer for a URI — a file on a share that has gone away, a
 * document the granting app has since revoked — and picking three
 * files to find two attached, with nothing said, is somebody being
 * quietly lied to about what is going out.
 */
data class ReadFiles(
    val attachments: List<OutgoingMessage.Attachment>,
    val lost: List<String>,
)
