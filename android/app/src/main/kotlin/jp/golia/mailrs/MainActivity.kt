package jp.golia.mailrs

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.material3.Surface
import androidx.compose.runtime.getValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import jp.golia.mailrs.ui.LocalTheme
import jp.golia.mailrs.ui.MailrsApp
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
        model?.useServer(url)
    }

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
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            // Not Material You: the three clients hold one palette, and
            // an accent taken from the wallpaper is not that. See
            // `ui/Theme.kt`.
            MailrsTheme {
                Surface(color = LocalTheme.current.bg) {
                    val vm: MailViewModel = viewModel()
                    // `am start … --es mailrs_base_url http://10.0.2.2:6039`
                    // is this app's `-mailrsBaseURL`. Ignored outside a
                    // debug build; see `MailViewModel.useServer`.
                    model = vm
                    vm.useServer(intent?.getStringExtra("mailrs_base_url"))
                    val state by vm.state.collectAsStateWithLifecycle()
                    MailrsApp(vm, state)
                }
            }
        }
    }
}
