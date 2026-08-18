package jp.golia.mailrs

import androidx.test.platform.app.InstrumentationRegistry

/**
 * Where the stub is, said once.
 *
 * The address was written out in five places — a base class, its
 * companion, two other classes and a helper — so a port change would
 * have been right in four of them and wrong in the fifth, which is the
 * kind of disagreement that reads as "one test cannot reach the
 * server".
 */
object StubServer {

    /**
     * Guest-local, reached through `adb reverse` — see
     * `scripts/android-build.sh`. Not `10.0.2.2`: that crosses the
     * emulator's NAT, and a suite's worth of short-lived connections
     * through it stalls a connect every so often, which arrives as one
     * unrelated test failing per run.
     */
    const val DEFAULT = "http://127.0.0.1:6039"

    /** What this run was told to use, or the default. */
    fun base(): String =
        InstrumentationRegistry.getArguments().getString("mailrsBaseURL") ?: DEFAULT
}
