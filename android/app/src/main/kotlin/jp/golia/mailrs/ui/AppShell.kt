package jp.golia.mailrs.ui

import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.width
import androidx.compose.material3.VerticalDivider
import androidx.compose.ui.Alignment
import androidx.compose.ui.platform.testTag
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.unit.dp
import jp.golia.mailrs.AccountDetail
import jp.golia.mailrs.UiState
import jp.golia.mailrs.closeAccount
import jp.golia.mailrs.closeAdmin
import jp.golia.mailrs.closeAdminRow
import jp.golia.mailrs.openAdmin
import jp.golia.mailrs.cancelCompose
import jp.golia.mailrs.closeDrafts
import jp.golia.mailrs.MailViewModel
import kotlin.coroutines.cancellation.CancellationException

/** Which screen is showing, in the order they stack. */
private enum class Screen {
    SignIn, List, Drafts, Settings, Admin, GroupDetail, AccountDetail, Thread, Source, Compose,
}

/**
 * The app's one navigation decision, and the motion that goes with it.
 *
 * **Screens used to swap with no transition at all**, which on Android
 * reads as a redraw rather than a move: nothing tells the eye whether it
 * went deeper or came back. Material's shared-axis pattern is the
 * platform's answer — forward enters from the right, back from the left.
 *
 * **Back is predictive.** From Android 14 the system draws the app
 * peeling back under the thumb while the gesture is still undecided, and
 * an app that only reacts on release is the one screen in the phone that
 * does not move until it is too late to change your mind. The progress
 * flow scales and slides the screen being left, and a cancelled gesture
 * snaps it back — the cancellation *is* the signal, which is why it is
 * caught rather than allowed to propagate.
 *
 * `PredictiveBackHandler` also serves an ordinary press: the dispatcher
 * completes the flow at once, so there is no separate `BackHandler` and
 * no chance of the two disagreeing about which screen to leave.
 */
@Composable
fun MailrsApp(vm: MailViewModel, state: UiState) {
    val windowSize = LocalWindowInfo.current.containerSize
    val density = LocalDensity.current
    val windowWidthDp = with(density) { windowSize.width.toDp() }
    val windowModifier = with(density) {
        Modifier.size(windowWidthDp, windowSize.height.toDp())
    }

    // A tablet, or a foldable that has been opened. The list keeps its
    // place and the message appears beside it rather than on top —
    // which is what the width is for, and the whole of Android's
    // large-screen guidance in one sentence.
    val twoPanes = Panes.twoPanes(windowWidthDp.value.toInt())


    // **This order is the stack, innermost first.** Each line asks "is
    // something on top of what the line below would show?", so a screen
    // opened *from* another must appear above it — and getting that
    // wrong does not look like a bug, it looks like the tap did
    // nothing. It has happened twice: Admin under Settings, and Source
    // under Thread, both of which left the screen unchanged because the
    // one underneath was still open.
    val screen = when {
        !state.signedIn -> Screen.SignIn
        // On top of whatever opened it, so cancelling returns there.
        state.composing != null -> Screen.Compose
        state.sourceOpen -> Screen.Source          // opened from a thread
        state.adminDetail != null -> Screen.GroupDetail    // opened from an admin list
        state.accountDetail != null -> Screen.AccountDetail // opened from the accounts list
        state.adminOpen != null -> Screen.Admin    // opened from settings
        state.settingsOpen -> Screen.Settings
        state.draftsOpen -> Screen.Drafts
        // With two panes the thread is *beside* the list rather than
        // over it, so it is not a screen of its own: List draws both.
        state.open != null && !twoPanes -> Screen.Thread
        else -> Screen.List
    }

    var backProgress by remember { mutableFloatStateOf(0f) }
    // Read at gesture time, not at registration time: the block below is
    // captured once and would otherwise leave whichever screen was
    // showing when it was composed.
    val current by rememberUpdatedState(screen)

    PredictiveBackHandler(
        enabled = screen == Screen.Thread || screen == Screen.Compose ||
            screen == Screen.Settings || screen == Screen.Drafts ||
            screen == Screen.Admin || screen == Screen.Source ||
            screen == Screen.GroupDetail || screen == Screen.AccountDetail,
    ) { progress ->
        try {
            progress.collect { backProgress = it.progress }
            backProgress = 0f
            when (current) {
                Screen.Compose -> vm.cancelCompose()
                Screen.Thread -> vm.closeThread()
                Screen.Settings -> vm.closeSettings()
                Screen.Drafts -> vm.closeDrafts()
                Screen.Admin -> vm.closeAdmin()
                Screen.Source -> vm.closeSource()
                Screen.GroupDetail -> vm.closeAdminRow()
                Screen.AccountDetail -> vm.closeAccount()
                else -> Unit
            }
        } catch (_: CancellationException) {
            // Let go without committing. The screen returns to where it
            // was, which is the whole point of showing the peek.
            backProgress = 0f
        }
    }

    // **A `Box`, not `AnimatedContent`.** The obvious tool measures its
    // children against an unbounded height while it works out what size
    // to animate to, and a child that fills what it is given then fills
    // infinity: the expanded search field came out 2400px tall and its
    // results were laid out at y=2574, off the bottom of a 2400px
    // screen. `using null` does not help — the measurement, not the size
    // animation, is what does it. Measured, not guessed: taking
    // `AnimatedContent` out and changing nothing else turned the search
    // tests green.
    //
    // Every screen is composed here and only the current one is visible,
    // so each is measured against the Box's real constraints.
    // **The window's size, in so many words.** `AnimatedVisibility`
    // measures its content against an unbounded height so the content
    // keeps its full size while the container animates — which means
    // `fillMaxSize()` inside it fills infinity. The expanded search
    // field came out 2400px tall and its results were laid out at
    // y=2574, off the bottom of a 2400px screen. Isolated by taking the
    // wrapper out and changing nothing else: the search tests went
    // green, and back red when it returned. A screen that is meant to
    // be exactly one window tall has to be told how tall that is.
    val previous = remember { intArrayOf(screen.ordinal) }
    val forward = screen.ordinal >= previous[0]
    previous[0] = screen.ordinal
    val enterFrom = if (forward) 1 else -1

    Box(Modifier.fillMaxSize()) {
        for (candidate in Screen.entries) {
            AnimatedVisibility(
                visible = screen == candidate,
                enter = slideInHorizontally { it * enterFrom } + fadeIn(),
                exit = slideOutHorizontally { -it * enterFrom / 4 } + fadeOut(),
                modifier = Modifier.fillMaxSize(),
            ) {
                val peel = backProgress
                val peeled = windowModifier
                    .graphicsLayer {
                        // The system's own shape: shrink a little, slide
                        // toward the edge the gesture came from, and round
                        // the corners so it reads as a card lifting off.
                        scaleX = 1f - 0.08f * peel
                        scaleY = 1f - 0.08f * peel
                        translationX = peel * 24.dp.toPx()
                    }
                    .clip(RoundedCornerShape((peel * 24f).dp))

                when (candidate) {
                    Screen.SignIn -> Box(windowModifier) {
                        SignInScreen(state.busy, state.error) { s, u, p -> vm.signIn(s, u, p) }
                    }
                    Screen.Compose -> Box(peeled) { ComposeScreen(state, vm) }
                    Screen.Thread -> Box(peeled) { ThreadScreen(state, vm) }
                    Screen.Source -> Box(peeled) { SourceScreen(state, vm) }
                    Screen.GroupDetail -> Box(peeled) {
                        state.adminDetail?.let { GroupDetailScreen(it, vm) }
                    }
                    Screen.AccountDetail -> Box(peeled) {
                        state.accountDetail?.let { AccountDetailScreen(it, vm) }
                    }
                    Screen.List -> Box(windowModifier) {
                        if (twoPanes) {
                            Row(Modifier.fillMaxSize()) {
                                Box(Modifier.width(Panes.LIST_PANE_WIDTH_DP.dp).fillMaxHeight()) {
                                    ConversationListScreen(state, vm)
                                }
                                VerticalDivider(color = LocalTheme.current.border, thickness = 0.5.dp)
                                Box(Modifier.weight(1f).fillMaxHeight().testTag("pane.detail")) {
                                    if (state.open != null) {
                                        ThreadScreen(state, vm)
                                    } else {
                                        // Not blank: a pane with nothing
                                        // in it and no explanation looks
                                        // like something failed to load.
                                        Conclusion(
                                            "No conversation open",
                                            "Choose one on the left.",
                                            Modifier.align(Alignment.Center),
                                        )
                                    }
                                }
                            }
                        } else {
                            ConversationListScreen(state, vm)
                        }
                    }
                    Screen.Drafts -> Box(peeled) { DraftsScreen(state, vm) }
                    Screen.Admin -> Box(peeled) {
                        // Non-null on this branch by construction; the
                        // screen is chosen from the same value.
                        state.adminOpen?.let { AdminScreen(it, state, vm) }
                    }
                    Screen.Settings -> Box(peeled) {
                        SettingsScreen(
                            state = state,
                            appearance = state.appearance,
                            onAppearance = { vm.chooseAppearance(it) },
                            onNotify = { vm.chooseNotify(it) },
                            onClose = { vm.closeSettings() },
                            onAdmin = { vm.openAdmin(it) },
                            onSignOut = {
                                vm.closeSettings()
                                vm.signOut()
                            },
                        )
                    }
                }
            }
        }
    }
}
