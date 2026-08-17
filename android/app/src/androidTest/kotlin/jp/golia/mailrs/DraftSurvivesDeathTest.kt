package jp.golia.mailrs

import androidx.lifecycle.SavedStateHandle
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

/**
 * A half-written message survives the app being taken away.
 *
 * A `ViewModel` outlives a rotation and nothing else: Android reclaims
 * a backgrounded app whenever it wants the memory — a phone call, a
 * camera, a large game — and brings it back looking as it did, which
 * for this app meant an empty composer where a message had been.
 * Leaving the composer saves a server draft, but being killed is not
 * leaving.
 *
 * **Process death itself is not simulated here.** No instrumented test
 * can take the process away and hand the same `SavedStateHandle` back;
 * what can be checked is both halves of the contract that makes it
 * work — that a draft is written to the handle as it is typed, and
 * that a view model built from that handle comes back holding it. The
 * gap between those halves is Android's own machinery.
 */
@RunWith(AndroidJUnit4::class)
class DraftSurvivesDeathTest {

    /**
     * No session, so nothing is fetched.
     *
     * A token left by another test makes a fresh view model start
     * requesting against whatever server that test pointed it at, and
     * this one is about the composer.
     */
    @Before
    fun signedOut() {
        val store = jp.golia.mailrs.wire.TokenStore(
            ApplicationProvider.getApplicationContext<android.content.Context>(),
        )
        store.clear()

    }

    private fun viewModel(saved: SavedStateHandle = SavedStateHandle()): MailViewModel {
        lateinit var vm: MailViewModel
        // On the main thread: `viewModelScope` lives on Dispatchers.Main
        // and the collector that keeps the handle up to date starts in
        // `init`.
        InstrumentationRegistry.getInstrumentation().runOnMainSync {
            vm = MailViewModel(ApplicationProvider.getApplicationContext(), saved)
        }
        return vm
    }

    /** Typing is kept, without anybody asking for it to be saved. */
    @Test
    fun a_draft_being_typed_reaches_the_saved_state() {
        val saved = SavedStateHandle()
        val vm = viewModel(saved)
        InstrumentationRegistry.getInstrumentation().runOnMainSync {
            vm.compose()
            vm.editDraft(to = "a@x.test", subject = "Half a thought", body = "the rest")
        }
        InstrumentationRegistry.getInstrumentation().waitForIdleSync()

        val stored = saved.get<String>(MailViewModel.DRAFT_KEY)
        assertNotNull("nothing was kept for a killed process", stored)
        assertEquals(true, stored!!.contains("Half a thought"))
        assertEquals(true, stored.contains("the rest"))
    }

    /** And a view model built from that handle opens on the message. */
    @Test
    fun a_recreated_view_model_opens_on_the_message() {
        val saved = SavedStateHandle()
        val first = viewModel(saved)
        InstrumentationRegistry.getInstrumentation().runOnMainSync {
            first.compose()
            first.editDraft(to = "a@x.test", subject = "Survives", body = "text")
        }
        InstrumentationRegistry.getInstrumentation().waitForIdleSync()

        // The same handle, a new view model: what Android does after it
        // has reclaimed the process.
        val second = viewModel(saved)
        val draft = second.state.value.composing
        assertNotNull("the composer came back empty", draft)
        assertEquals("Survives", draft!!.subject)
        assertEquals("a@x.test", draft.to)
    }

    /** Nothing is kept once the composer is closed. */
    @Test
    fun leaving_the_composer_clears_what_was_kept() {
        val saved = SavedStateHandle()
        val vm = viewModel(saved)
        InstrumentationRegistry.getInstrumentation().runOnMainSync {
            vm.compose()
            vm.editDraft(subject = "gone in a moment")
            vm.cancelCompose()
        }
        InstrumentationRegistry.getInstrumentation().waitForIdleSync()
        assertNull(saved.get<String>(MailViewModel.DRAFT_KEY))
    }
}
