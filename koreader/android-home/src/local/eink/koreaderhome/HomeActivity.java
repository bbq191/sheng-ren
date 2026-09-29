package local.eink.koreaderhome;

import android.app.Activity;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.os.Bundle;
import android.os.SystemClock;
import android.widget.Toast;

import java.util.List;

/**
 * 桌面（HOME）入口：系统要"回桌面"时（开机、按主页键）打开 KOReader，自己不显示任何界面。
 * 两秒内连按两次主页键 → 打开别的桌面（掌阅原来的），用来进系统设置、把默认桌面改回去。
 * 找不到 KOReader 时也打开别的桌面，不会卡死在这里。
 */
public class HomeActivity extends Activity {
    /** 上一次按主页键的时间（开机以来的毫秒）；进程还在就一直记着。 */
    private static long lastHome = 0;
    private static final long DOUBLE_PRESS_MS = 2000;
    private static final String[] KOREADER = {"org.koreader.launcher", "org.koreader.launcher.fdroid"};

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        go();
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        go();
    }

    private void go() {
        long now = SystemClock.elapsedRealtime();
        boolean twice = lastHome != 0 && now - lastHome < DOUBLE_PRESS_MS;
        lastHome = twice ? 0 : now;
        if (twice || !openKOReader()) {
            openOtherHome();
        }
        finish();
    }

    /** 打开 KOReader（已经开着就切回去，读到的位置不丢）。 */
    private boolean openKOReader() {
        PackageManager pm = getPackageManager();
        for (String pkg : KOREADER) {
            Intent i = pm.getLaunchIntentForPackage(pkg);
            if (i != null) {
                return start(i);
            }
        }
        // 包名不在上面（别的构建）：在所有可启动的应用里找名字带 koreader 的
        Intent all = new Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER);
        for (ResolveInfo r : pm.queryIntentActivities(all, 0)) {
            if (r.activityInfo.packageName.toLowerCase().contains("koreader")) {
                Intent i = pm.getLaunchIntentForPackage(r.activityInfo.packageName);
                if (i != null) {
                    return start(i);
                }
            }
        }
        return false;
    }

    /** 打开除自己以外的第一个桌面（掌阅桌面）。 */
    private void openOtherHome() {
        Intent home = new Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_HOME);
        List<ResolveInfo> homes = getPackageManager().queryIntentActivities(home, 0);
        for (ResolveInfo r : homes) {
            if (!r.activityInfo.packageName.equals(getPackageName())) {
                Intent i = new Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_HOME);
                i.setClassName(r.activityInfo.packageName, r.activityInfo.name);
                if (start(i)) {
                    return;
                }
            }
        }
        Toast.makeText(this, "没找到别的桌面", Toast.LENGTH_LONG).show();
    }

    private boolean start(Intent i) {
        i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        try {
            startActivity(i);
            return true;
        } catch (RuntimeException e) {
            return false;
        }
    }
}
