package local.eink.koreaderhome;

import android.app.Activity;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.os.Bundle;
import android.provider.Settings;
import android.widget.Toast;

/**
 * 掌阅桌面上的图标「KOReader 独占」：回到原生系统（默认桌面改成了 iReader 桌面）以后，点它重回独占（用户 2026-10-02）。
 * - 「KOReader 桌面」已经是默认桌面：直接打开 KOReader。
 * - 不是：打开系统的"默认桌面"设置页，提示选「KOReader 桌面」；选好后系统回桌面，就进了 KOReader。
 *   安卓不许普通应用自己改默认桌面，只能把用户带到设置页。掌阅没有这个页面时退到"默认应用"页，再不行退到设置首页。
 */
public class ExclusiveActivity extends Activity {
    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        if (isDefaultHome()) {
            startActivity(new Intent(this, HomeActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
        } else {
            String[] pages = {Settings.ACTION_HOME_SETTINGS, "android.settings.MANAGE_DEFAULT_APPS_SETTINGS", Settings.ACTION_SETTINGS};
            for (String page : pages) {
                if (open(page)) {
                    break;
                }
            }
            Toast.makeText(this, "在「桌面」里选「KOReader 桌面」，就回到 KOReader 独占", Toast.LENGTH_LONG).show();
        }
        finish();
    }

    /** 现在的默认桌面是不是自己。 */
    private boolean isDefaultHome() {
        Intent home = new Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_HOME);
        ResolveInfo r = getPackageManager().resolveActivity(home, PackageManager.MATCH_DEFAULT_ONLY);
        return r != null && r.activityInfo != null && getPackageName().equals(r.activityInfo.packageName);
    }

    /** 直接试着打开，打不开（没有这个页面）再换下一个；不先查 resolveActivity：安卓 11 起的应用可见性规则会让它误报"没有"。 */
    private boolean open(String action) {
        Intent i = new Intent(action).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        try {
            startActivity(i);
            return true;
        } catch (RuntimeException e) {
            return false;
        }
    }
}
