package jp.golia.mailrs

import jp.golia.mailrs.openThreadById
import jp.golia.mailrs.compose
import jp.golia.mailrs.composeFromShare
import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.os.Parcelable
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.activity.enableEdgeToEdge
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.getValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import jp.golia.mailrs.ui.LocalTheme
import androidx.compose.runtime.LaunchedEffect
import jp.golia.mailrs.ui.MailrsApp
import jp.golia.mailrs.wire.NewMailWorker
import jp.golia.mailrs.wire.ShareIntent
import jp.golia.mailrs.wire.Prefs
import jp.golia.mailrs.ui.MailrsTheme

class MainActivity : ComponentActivity() {

    /**
     * The view model this activity is showing, so an instrumented test
     * can point it at the stub after launch — the rule launches the
     * activity itself, so there is no intent to put an extra on.
     *
     * `useServer` is the thing that refuses outside a debug build; this
     * only hands it the string.
     */
    private var model: MailViewModel? = null

    fun useStubServer(url: String) {
        // Remembered, not dropped. This used to be `model?.useServer(…)`
        // and did nothing at all when the composition had not run yet —
        // so the sign-in that followed went to the real host and failed
        // with "Failure in SSL library", which reads as a network
        // problem and is really a test hook that missed.
        pendingServer = url
        model?.useServer(url)
    }

    private var pendingServer: String? = null

    /**
     * Start from signed-out, the way the iOS suite's
     * `-mailrsFreshCache` does.
     *
     * A run that signs in leaves a token behind, so the next run opens
     * on the inbox and every sign-in test fails looking for a field that
     * is not on screen. That is what happened the first time this suite
     * ran twice: three tests went red with "could not find
     * TestTag = 'field.address'", which reads as a broken locator and is
     * really yesterday's session.
     */
    fun signOutForTest() {
        model?.signOut()
    }

    /**
     * Deliver an intent without going through the activity lifecycle.
     *
     * Calling `onNewIntent` from a test upsets `ActivityScenario`, which
     * then waits for a DESTROYED that never comes. This is the same
     * work with the lifecycle left alone; that the *filter* exists is
     * asserted separately, by asking the package manager who resolves
     * `mailto:`.
     */
    fun deliverForTest(intent: Intent) {
        if (!BuildConfig.ALLOW_SERVER_OVERRIDE) return
        model?.let { actOn(it, intent) }
    }

    /** Point the list at a stub-only folder — see `useFolderForTest`. */
    fun useFolderForTest(folder: String) {
        model?.useFolderForTest(folder)
    }

    /** A cold launch without the launch — see `forgetLoadedMail`. */
    fun forgetLoadedMailForTest() {
        model?.forgetLoadedMail()
    }

    /**
     * Attach a file without the system picker.
     *
     * The picker runs in another process and is the platform's, not
     * this app's; what is worth testing is everything after it — the
     * name and size read from the resolver, the streaming body, and the
     * multipart shape the server reads. Debug builds only, for the same
     * reason `useServer` is.
     */
    fun attachForTest(uri: android.net.Uri) {
        if (!BuildConfig.ALLOW_SERVER_OVERRIDE) return
        model?.attach(listOf(uri))
    }
    /**
     * A new intent while this activity is already up — a second share,
     * or a shortcut tapped from the recents screen. Without this the
     * activity keeps the one it launched with and the share appears to
     * have done nothing.
     */
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        model?.let { actOn(it, intent) }
    }

    /**
     * Do what another app asked.
     *
     * `mailto:` from a link anywhere on the phone, the share sheet with
     * text or files, and the two launcher shortcuts. Everything lands
     * in a draft, so leaving still saves it.
     */
    private fun actOn(vm: MailViewModel, intent: Intent?) {
        // A tapped notification, which arrives as an ordinary launch
        // carrying an extra rather than as an action of its own.
        intent?.getStringExtra(NewMailWorker.EXTRA_THREAD_ID)?.let { threadId ->
            intent.removeExtra(NewMailWorker.EXTRA_THREAD_ID)
            vm.openThreadById(threadId)
            return
        }
        when (intent?.action) {
            Intent.ACTION_VIEW, Intent.ACTION_SENDTO -> {
                val uri = intent.data?.toString() ?: return
                if (!uri.startsWith("mailto:")) return
                vm.composeFromShare(mailto = ShareIntent.mailto(uri))
            }

            Intent.ACTION_SEND, Intent.ACTION_SEND_MULTIPLE -> {
                val files = buildList {
                    intent.getParcelableExtraCompat<android.net.Uri>(Intent.EXTRA_STREAM)?.let(::add)
                    addAll(intent.getParcelableArrayListExtraCompat(Intent.EXTRA_STREAM))
                }
                vm.composeFromShare(
                    subject = intent.getStringExtra(Intent.EXTRA_SUBJECT).orEmpty(),
                    body = intent.getStringExtra(Intent.EXTRA_TEXT).orEmpty(),
                    attachments = files,
                )
            }

            ACTION_COMPOSE -> vm.compose()
            ACTION_SEARCH -> vm.openSearchFromShortcut()
        }
    }

    /**
     * Asked for once signed in, which is the moment it means something.
     *
     * At first launch it would be a prompt about nothing — there is no
     * mailbox yet — and Android only shows the system dialog once, so
     * spending it on a screen where the answer is "what mail?" spends
     * it badly. Refusing costs only the notification.
     */
    private val askNotifications = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { /* Granted or not, the app is unchanged; the check just stays quiet. */ }

    override fun onCreate(savedInstanceState: Bundle?) {
        // Before `super`, which is where the API requires it: it swaps
        // the launch theme for the real one, and after super.onCreate
        // the window has already been made with the wrong one.
        installSplashScreen()
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        NewMailWorker.ensureChannel(this)
        setContent {
            // Not Material You: the three clients hold one palette, and
            // an accent taken from the wallpaper is not that. See
            // `ui/Theme.kt`.
            val vm: MailViewModel = viewModel()
            val state by vm.state.collectAsStateWithLifecycle()
            // "Follow the phone" is a real answer and the default one, so
            // the preference resolves to a boolean here rather than being
            // stored as one — a two-state switch freezes whichever mode
            // the phone happened to be in.
            val dark = when (state.appearance) {
                Prefs.Appearance.System -> isSystemInDarkTheme()
                Prefs.Appearance.Light -> false
                Prefs.Appearance.Dark -> true
            }
            MailrsTheme(dark = dark) {
                Surface(color = LocalTheme.current.bg) {
                    // `am start … --es mailrs_base_url http://10.0.2.2:6039`
                    // is this app's `-mailrsBaseURL`. Ignored outside a
                    // debug build; see `MailViewModel.useServer`.
                    model = vm
                    vm.useServer(pendingServer ?: intent?.getStringExtra("mailrs_base_url"))
                    // Once per intent, not once per recomposition: the
                    // key is the intent itself, so a rotation does not
                    // reopen a composer the person just cancelled.
                    LaunchedEffect(intent) { actOn(vm, intent) }

                    // Signed in: start checking, and ask for the
                    // permission that lets the check say anything.
                    LaunchedEffect(state.signedIn) {
                        if (!state.signedIn) return@LaunchedEffect
                        NewMailWorker.schedule(this@MainActivity, state.notifyNewMail)
                        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
                            ContextCompat.checkSelfPermission(
                                this@MainActivity,
                                Manifest.permission.POST_NOTIFICATIONS,
                            ) != PackageManager.PERMISSION_GRANTED
                        ) {
                            askNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
                        }
                    }
                    MailrsApp(vm, state)
                }
            }
        }
    }

    private companion object {
        const val ACTION_COMPOSE = "jp.golia.mailrs.COMPOSE"
        const val ACTION_SEARCH = "jp.golia.mailrs.SEARCH"
    }
}

/**
 * `getParcelableExtra` without the deprecation, and without losing the
 * older devices: the typed overload arrives in API 33 and this app
 * supports 29.
 */
private inline fun <reified T : Parcelable> Intent.getParcelableExtraCompat(name: String): T? =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        getParcelableExtra(name, T::class.java)
    } else {
        @Suppress("DEPRECATION")
        getParcelableExtra(name) as? T
    }

private inline fun <reified T : Parcelable> Intent.getParcelableArrayListExtraCompat(
    name: String,
): List<T> =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        getParcelableArrayListExtra(name, T::class.java).orEmpty()
    } else {
        @Suppress("DEPRECATION")
        getParcelableArrayListExtra<T>(name).orEmpty()
    }
