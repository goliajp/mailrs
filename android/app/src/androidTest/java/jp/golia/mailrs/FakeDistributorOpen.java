package jp.golia.mailrs;

import android.app.Activity;
import android.os.Bundle;

/**
 * Opening the fake distributor once, as a person opens a distributor app
 * after installing it. A package never opened is in the stopped state, and
 * the connector's registration broadcast does not reach it.
 */
public class FakeDistributorOpen extends Activity {
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        finish();
    }
}
