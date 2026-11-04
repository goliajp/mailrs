package jp.golia.mailrs.wire

/**
 * What the reader's image setting means for one message on screen; the web's
 * `mailrs_remote_images` and the iPhone's `Wire/RemoteImages.swift`.
 *
 * `Auto` loads remote images with the message; `Click` holds them until
 * "Load" is tapped; `Never` holds them and offers no button. The tap is per
 * message and not remembered, whatever the setting.
 */
enum class RemoteImages {
    Auto, Click, Never;

    fun blocked(loaded: Boolean): Boolean = when (this) {
        Auto -> false
        Click -> !loaded
        Never -> true
    }

    fun offersButton(loaded: Boolean): Boolean = this == Click && !loaded
}
