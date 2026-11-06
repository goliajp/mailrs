package jp.golia.mailrs;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;

/**
 * A UnifiedPush distributor, as far as the app can tell: it answers a
 * registration with an endpoint, by the broadcasts of the UnifiedPush
 * Android specification. It runs in the test APK's own process, so the app
 * talks to it across processes as it would to a real distributor app.
 *
 * Java, not Kotlin: in its own process the test APK has no Kotlin standard
 * library, which it otherwise borrows from the app it tests.
 *
 * The connector's token is put into the endpoint, which is how a test
 * learns it from what the app sent the server, and can then push a
 * message the way a distributor does ({@link #ping}).
 */
public class FakeDistributor extends BroadcastReceiver {
    public static final String ENDPOINT_PREFIX = "https://push.example.org/up-";
    private static final String REGISTER = "org.unifiedpush.android.distributor.REGISTER";
    private static final String NEW_ENDPOINT = "org.unifiedpush.android.connector.NEW_ENDPOINT";
    private static final String MESSAGE = "org.unifiedpush.android.connector.MESSAGE";

    @Override
    public void onReceive(Context context, Intent intent) {
        if (!REGISTER.equals(intent.getAction())) return;
        String token = intent.getStringExtra("token");
        String app = intent.getStringExtra("application");
        if (token == null || app == null) return;
        context.sendBroadcast(
            new Intent(NEW_ENDPOINT)
                .setPackage(app)
                .putExtra("token", token)
                .putExtra("endpoint", ENDPOINT_PREFIX + token));
    }

    /** Deliver {@code body} to {@code app} for the registration {@code endpoint} names. */
    public static void ping(Context context, String app, String endpoint, byte[] body) {
        context.sendBroadcast(
            new Intent(MESSAGE)
                .setPackage(app)
                .putExtra("token", endpoint.substring(ENDPOINT_PREFIX.length()))
                .putExtra("bytesMessage", body));
    }
}
