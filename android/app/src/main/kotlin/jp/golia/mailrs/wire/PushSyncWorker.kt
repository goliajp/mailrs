package jp.golia.mailrs.wire

import android.content.Context
import android.os.Build
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import androidx.work.workDataOf

/**
 * Tells the server what the distributor gave: registers the endpoint, or
 * drops it. A worker rather than a call from the push service, because the
 * distributor hands over endpoints whenever it likes, offline included.
 *
 * Asked to [check], it first reads what the server lists and registers this
 * phone again only if the server no longer has it.
 */
class PushSyncWorker(
    context: Context,
    params: WorkerParameters,
) : CoroutineWorker(context, params) {

    override suspend fun doWork(): Result {
        val reg = PushRegistration(applicationContext)
        val client = MailrsClient(TokenStore(applicationContext))
        if (client.session == null) return Result.success()
        val checking = inputData.getBoolean(CHECK, false)
        val steps = if (checking) checked(client, reg.snapshot()) else PushSync.steps(reg.snapshot())
        for (step in steps) {
            val outcome = when (step) {
                is PushSync.Step.Drop -> drop(client, reg, step)
                is PushSync.Step.Register -> register(client, reg, step)
                PushSync.Step.TurnOff -> Done.RemovedElsewhere
            }
            when (outcome) {
                Done.Ok -> Unit
                Done.RemovedElsewhere -> {
                    PushDistributor.removedElsewhere(applicationContext)
                    return Result.success()
                }
                is Done.Failed -> {
                    reg.failure = outcome.reason
                    return again()
                }
            }
        }
        if (checking && steps.any { it is PushSync.Step.Register }) reg.reregistered(System.currentTimeMillis())
        return Result.success()
    }

    private sealed interface Done {
        data object Ok : Done
        data object RemovedElsewhere : Done
        data class Failed(val reason: String) : Done
    }

    private suspend fun checked(client: MailrsClient, have: PushSync.Snapshot): List<PushSync.Step> {
        // off or not yet told: nothing to compare, and no reason to ask
        if (!PushSync.registered(have)) return emptyList()
        val listed = when (val r = client.unifiedPushState()) {
            is MailrsClient.Outcome.Ok -> r.value
            is MailrsClient.Outcome.Err -> null
        }
        return PushSync.recheck(have, listed)
    }

    private suspend fun drop(client: MailrsClient, reg: PushRegistration, step: PushSync.Step.Drop): Done =
        when (val r = client.unregisterUnifiedPush(step.id)) {
            is MailrsClient.Outcome.Ok -> {
                reg.dropped()
                Done.Ok
            }
            is MailrsClient.Outcome.Err -> Done.Failed(r.message)
        }

    private suspend fun register(client: MailrsClient, reg: PushRegistration, step: PushSync.Step.Register): Done {
        val body = UnifiedPushWire.Register(
            step.endpoint,
            UnifiedPushWire.Keys(step.p256dh, step.auth),
            deviceName(),
            resume = reg.turnedOn,
        )
        return when (val r = client.registerUnifiedPush(body)) {
            is MailrsClient.Outcome.Ok -> {
                reg.registered(r.value.id, step.endpoint, PushSync.sent(step.endpoint, step.p256dh, step.auth))
                reg.turnedOn = false
                Done.Ok
            }
            // asking again would be refused the same way, and is what put it back before
            is MailrsClient.Outcome.Err if r.code == UnifiedPushWire.REMOVED -> Done.RemovedElsewhere
            is MailrsClient.Outcome.Err -> Done.Failed(r.message)
        }
    }

    // a refusal does not change by asking again forever; the network might
    private fun again(): Result {
        if (runAttemptCount < MAX_ATTEMPTS) return Result.retry()
        return Result.failure()
    }

    companion object {
        const val WORK_NAME = "unifiedpush-sync"
        private const val CHECK = "check"
        private const val MAX_ATTEMPTS = 5

        /** What the server lists this phone as. */
        fun deviceName(): String = Build.MODEL.take(64)

        fun enqueue(context: Context) = start(context, check = false)

        /** Make sure the server still has this phone, and register it again if not. */
        fun check(context: Context) = start(context, check = true)

        private fun start(context: Context, check: Boolean) {
            WorkManager.getInstance(context).enqueueUniqueWork(
                WORK_NAME,
                ExistingWorkPolicy.APPEND_OR_REPLACE,
                OneTimeWorkRequestBuilder<PushSyncWorker>()
                    .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                    .setInputData(workDataOf(CHECK to check))
                    .build(),
            )
        }
    }
}
