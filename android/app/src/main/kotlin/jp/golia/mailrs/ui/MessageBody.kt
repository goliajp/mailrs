package jp.golia.mailrs.ui

import android.content.Intent
import android.graphics.Color as AndroidColor
import android.view.ViewGroup
import android.webkit.CookieManager
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.wire.MailAppearance
import jp.golia.mailrs.wire.RemoteContent

/**
 * An email body, rendered rather than reinterpreted.
 *
 * A `WebView`, for the reason `ios/Mailrs/Features/MessageBodyView.swift`
 * gives: mail is arbitrary HTML with tables and absolute widths, and
 * anything that parses it into native views shows something the sender
 * did not write. What was here before stripped the tags with a regular
 * expression, which reads a newsletter as a column of disconnected
 * words.
 *
 * **Nothing is fetched until the reader asks.** `blockNetworkLoads`
 * refuses every subresource, so a tracking pixel — and a logo, which
 * reports identically — cannot tell the sender the message was opened,
 * from which address, at what time, on what network. The banner appears
 * only for mail that actually has something to load, which is what
 * `RemoteContent` answers.
 *
 * The rest of the hardening is the same list, in Android's vocabulary:
 *
 * - **JavaScript off.** Nothing in an email needs to run code.
 * - **No file or content access.** Without this, mail HTML can read
 *   `file://` and `content://` URIs — the phone's own storage — and
 *   with images enabled that is an exfiltration path, not a rendering
 *   feature.
 * - **No cookies.** A tracking cookie set by one message must not be
 *   readable by the next.
 * - **`baseUrl = null`.** Relative URLs then resolve to nothing rather
 *   than to some host the document names.
 * - **Links open outside.** A tap leaves for the browser; the message
 *   view never navigates, so a link cannot replace the mail with a
 *   page that looks like the app.
 *
 * Fitting is the platform's: `useWideViewPort` plus `loadWithOverviewMode`
 * is Android's own answer to "this page was authored at 640px and the
 * phone is not", and it is the same answer `web/src/lib/fit-to-width.ts`
 * computes by hand. The sender's own `<meta viewport>` is dropped first,
 * or it fights the one that makes this work.
 */
@Composable
fun MessageBody(html: String, modifier: Modifier = Modifier) {
    val theme = LocalTheme.current
    val dark = theme.isDark && MailAppearance.followsAppTheme(html)
    // Saveable: somebody who asked for the images should not have to
    // ask again because they turned the phone. Keyed on the message, so
    // the next one starts blocked again — the decision is per message,
    // not a setting.
    var blockRemote by rememberSaveable(html) { mutableStateOf(true) }
    val hasRemote = remember(html) { RemoteContent.hasRemoteReferences(html) }

    if (hasRemote && blockRemote) {
        Row(
            Modifier
                .fillMaxWidth()
                .padding(bottom = 8.dp)
                .background(theme.warning.copy(alpha = 0.12f), RoundedCornerShape(8.dp))
                .padding(start = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                "Images not loaded",
                color = theme.fgSecondary,
                fontSize = 12.sp,
                modifier = Modifier.testTag("body.remoteBlocked"),
            )
            TextButton(
                onClick = { blockRemote = false },
                modifier = Modifier.testTag("button.loadImages"),
            ) {
                Text("Load", color = theme.accent, fontSize = 12.sp)
            }
        }
    }

    val document = remember(html, dark, theme) { documentFor(html, dark, theme) }
    // What this WebView has already been given. Not a View tag: those
    // take an application resource id and throw on anything else, and
    // not Compose state either, because changing it must not recompose.
    val loaded = remember { arrayOfNulls<String>(1) }

    AndroidView(
        modifier = modifier.fillMaxWidth().testTag("body.web"),
        factory = { context ->
            WebView(context).apply {
                layoutParams = ViewGroup.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.WRAP_CONTENT,
                )
                CookieManager.getInstance().setAcceptThirdPartyCookies(this, false)
                setBackgroundColor(AndroidColor.TRANSPARENT)
                isVerticalScrollBarEnabled = false
                with(settings) {
                    javaScriptEnabled = false
                    allowFileAccess = false
                    allowContentAccess = false
                    domStorageEnabled = false
                    useWideViewPort = true
                    loadWithOverviewMode = true
                    builtInZoomControls = true
                    displayZoomControls = false
                }
                webViewClient = object : WebViewClient() {
                    // The body never navigates. A link that replaced the
                    // message with a page would let a phishing mail draw
                    // the app's own screen inside the app's own frame.
                    override fun shouldOverrideUrlLoading(
                        view: WebView,
                        request: WebResourceRequest,
                    ): Boolean {
                        runCatching {
                            context.startActivity(
                                Intent(Intent.ACTION_VIEW, request.url)
                                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                            )
                        }
                        return true
                    }
                }
            }
        },
        update = { web ->
            // Set before the load, never after: a block installed once
            // the page is up arrives after the beacon has fired, which
            // is the whole thing it prevents.
            web.settings.blockNetworkLoads = blockRemote
            val key = document + "\u0000" + blockRemote
            if (loaded[0] != key) {
                loaded[0] = key
                web.loadDataWithBaseURL(null, document, "text/html", "utf-8", null)
            }
        },
    )
}

/**
 * The message wrapped in a document that pins what email cannot be
 * trusted to get right.
 *
 * Mail that declares its own colours is rendered on white paper whatever
 * the phone's appearance — honouring dark for a message that sets black
 * text produces black on black, which is worse than a bright rectangle.
 * Mail that declares none follows the app, because for a paragraph and a
 * link the bright rectangle is the only thing wrong with the screen.
 */
private fun documentFor(html: String, dark: Boolean, theme: Theme): String {
    val paper = if (dark) theme.surface.toArgb().rgbHex() else "#ffffff"
    val ink = if (dark) theme.fg.toArgb().rgbHex() else "#111111"
    val link = theme.accent.toArgb().rgbHex()
    // The sender's viewport would fight the one below, which is what
    // makes an email authored at 640px fit a phone at all.
    val body = html.replace(Regex("(?i)<meta[^>]*name=[\"']?viewport[\"']?[^>]*>"), "")
    return """
        <!doctype html><html><head>
        <meta charset="utf-8">
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <style>
          html,body{margin:0;padding:0;background:$paper;color:$ink;
            font:15px/1.5 -apple-system,Roboto,sans-serif;
            word-break:break-word;overflow-wrap:anywhere;}
          img{max-width:100%;height:auto;}
          a{color:$link;}
          pre{white-space:pre-wrap;}
          /* **Anything with a width, not just tables.** A newsletter
             is a 760px table with a 760px div inside it, and
             constraining only the table left the div to run off the
             right edge — every line cut mid-word, at any font size and
             unmissably at 200%. `width:auto` overrides the inline
             attribute; the max-width keeps what is narrower narrow. */
          table,div,td,th,section,article{max-width:100% !important;}
          [width],[style*="width"]{max-width:100% !important;}
          table,div{width:auto !important;}
        </style></head><body>$body</body></html>
    """.trimIndent()
}

private fun Int.rgbHex(): String = String.format("#%06X", 0xFFFFFF and this)
