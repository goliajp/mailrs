package jp.golia.mailrs

import android.app.Application
import jp.golia.mailrs.wire.Texts

/**
 * The process, so the app's words are there before anything asks for
 * them: a worker or a widget can run with no activity at all.
 */
class MailrsApp : Application() {
    override fun onCreate() {
        super.onCreate()
        Texts.init(this)
    }
}
