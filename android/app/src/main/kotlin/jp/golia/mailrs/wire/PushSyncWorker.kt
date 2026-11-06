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

/**
 * Tells the server what the distributor gave: registers the endpoint, or
 * drops it. A worker rather than a call from the push service, because the
 * distributor hands over endpoints whenever it likes, offline included.
 */
class PushSyncWorker(
    context: Context,
    params: WorkerParameters,
) : CoroutineWorker(context, params) {

    override suspend fun doWork(): Result {
        val reg = PushRegistration(applicationContext)
        val client = MailrsClient(TokenStore(applicationContext))
        if (client.session == null) return Result.success()
        for (step in PushSync.steps(reg.snapshot())) {
            val failed = when (step) {
                is PushSync.Step.Drop -> drop(client, reg, step)
                is PushSync.Step.Register -> register(client, reg, step)
            }
            if (failed != null) {
                reg.failure = failed
                return again()
            }
        }
        return Result.success()
    }

    private suspend fun drop(client: MailrsClient, reg: PushRegistration, step: PushSync.Step.Drop): String? =
        when (val r = client.unregisterUnifiedPush(step.id)) {
            is MailrsClient.Outcome.Ok -> {
                reg.dropped()
                null
            }
            is MailrsClient.Outcome.Err -> r.message
        }

    private suspend fun register(client: MailrsClient, reg: PushRegistration, step: PushSync.Step.Register): String? {
        val body = UnifiedPushWire.Register(step.endpoint, UnifiedPushWire.Keys(step.p256dh, step.auth), deviceName())
        return when (val r = client.registerUnifiedPush(body)) {
            is MailrsClient.Outcome.Ok -> {
                reg.registered(r.value.id, step.endpoint, PushSync.sent(step.endpoint, step.p256dh, step.auth))
                null
            }
            is MailrsClient.Outcome.Err -> r.message
        }
    }

    // a refusal does not change by asking again forever; the network might
    private fun again(): Result {
        if (runAttemptCount < MAX_ATTEMPTS) return Result.retry()
        return Result.failure()
    }

    companion object {
        private const val WORK_NAME = "unifiedpush-sync"
        private const val MAX_ATTEMPTS = 5

        /** What the server lists this phone as. */
        fun deviceName(): String = Build.MODEL.take(64)

        fun enqueue(context: Context) {
            WorkManager.getInstance(context).enqueueUniqueWork(
                WORK_NAME,
                ExistingWorkPolicy.APPEND_OR_REPLACE,
                OneTimeWorkRequestBuilder<PushSyncWorker>()
                    .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                    .build(),
            )
        }
    }
}
