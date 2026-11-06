package jp.golia.mailrs;

import android.content.BroadcastReceiver;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;

/**
 * Takes {@link FakeDistributor} off the phone and puts it back, so a test can
 * see the app with no distributor installed. Only the test package may
 * change its own components (the shell may not), so the test asks this
 * receiver, which runs in that package's process.
 */
public class FakeDistributorSwitch extends BroadcastReceiver {
    public static final String ACTION = "jp.golia.mailrs.test.FAKE_DISTRIBUTOR";
    public static final String EXTRA_ON = "on";

    @Override
    public void onReceive(Context context, Intent intent) {
        int state = PackageManager.COMPONENT_ENABLED_STATE_DISABLED;
        if (intent.getBooleanExtra(EXTRA_ON, true)) state = PackageManager.COMPONENT_ENABLED_STATE_DEFAULT;
        context.getPackageManager().setComponentEnabledSetting(
            new ComponentName(context, FakeDistributor.class), state, PackageManager.DONT_KILL_APP);
    }

    /** Ask the test package to turn the fake distributor on or off. */
    public static void set(Context context, boolean on) {
        context.sendBroadcast(
            new Intent(ACTION)
                .setClassName(context.getPackageName(), FakeDistributorSwitch.class.getName())
                .putExtra(EXTRA_ON, on));
    }
}
