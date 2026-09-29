package local.eink.koreaderhome;

import android.app.Activity;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.os.Bundle;
import android.os.SystemClock;
import android.widget.Toast;

import java.util.List;

/**
 * 桌面（HOME）入口：每次开机后第一次"回桌面"（也就是开机）打开 KOReader；之后再回桌面（在 KOReader 里退出等）打开别的桌面
 * （掌阅原来的）。掌阅没有主页键（2026-09-29），不能靠按键逃生，所以做成"每次开机只自动打开一次"，和 Kindle 上一样。
 * 这次开机打开过没有，存在自己的设置里（按"开机时刻"认），进程被系统杀掉也不会重复打开。
 * 找不到 KOReader 时也打开别的桌面，不会卡在这里。自己不显示任何界面。
 */
public class HomeActivity extends Activity {
    private static final String[] KOREADER = {"org.koreader.launcher", "org.koreader.launcher.fdroid"};
    private static final String PREFS = "boot";
    private static final String KEY_BOOT = "koreader_opened_boot_ms";
    /** 两次算出来的"开机时刻"相差这么多以内算同一次开机（currentTimeMillis 会被网络校时微调）。 */
    private static final long SAME_BOOT_MS = 60_000;

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
        long bootAt = System.currentTimeMillis() - SystemClock.elapsedRealtime();
        SharedPreferences prefs = getSharedPreferences(PREFS, MODE_PRIVATE);
        long opened = prefs.getLong(KEY_BOOT, 0);
        boolean openedThisBoot = Math.abs(bootAt - opened) < SAME_BOOT_MS;
        if (!openedThisBoot && openKOReader()) {
            prefs.edit().putLong(KEY_BOOT, bootAt).apply();
        } else {
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
