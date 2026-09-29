package local.eink.koreaderhome;

import android.app.Activity;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.os.Bundle;
import android.os.SystemClock;
import android.provider.Settings;
import android.widget.Toast;

import java.util.List;

/**
 * 桌面（HOME）入口：每次开机后第一次"回桌面"（也就是开机）打开 KOReader；之后再回桌面（在 KOReader 里退出等）打开别的桌面
 * （掌阅原来的）。掌阅没有主页键（2026-09-29），不能靠按键逃生，所以做成"每次开机只自动打开一次"，和 Kindle 上一样。
 * 这次开机打开过没有，按系统的开机次数记在自己的设置里，进程被系统杀掉也不会重复打开；开机 3 分钟后一律进别的桌面。
 * 找不到 KOReader 时也打开别的桌面，不会卡在这里。自己不显示任何界面。
 */
public class HomeActivity extends Activity {
    private static final String[] KOREADER = {"org.koreader.launcher", "org.koreader.launcher.fdroid"};
    private static final String PREFS = "boot";
    private static final String KEY_BOOT = "koreader_opened_boot";
    /** 开机超过这么久就不再自动打开 KOReader（保险：万一认不出"这次开机打开过"，也不会一直把人关在 KOReader 里）。 */
    private static final long BOOT_WINDOW_MS = 3 * 60_000;
    private static boolean openedInProcess = false;

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
        String boot = bootId();
        SharedPreferences prefs = getSharedPreferences(PREFS, MODE_PRIVATE);
        // 取不到开机次数时只能靠进程里的记号（进程被杀会丢，但还有开机 3 分钟的窗口兜底）
        boolean openedThisBoot = boot == null ? openedInProcess : boot.equals(prefs.getString(KEY_BOOT, ""));
        boolean justBooted = SystemClock.elapsedRealtime() < BOOT_WINDOW_MS;
        if (!openedThisBoot && justBooted) {
            // 先记下再打开：就算打开时进程被杀，下次回桌面也不会再开
            openedInProcess = true;
            if (boot != null) {
                prefs.edit().putString(KEY_BOOT, boot).commit();
            }
            if (openKOReader()) {
                finish();
                return;
            }
        }
        openOtherHome();
        finish();
    }

    /**
     * 这次开机的标识（取不到返回 null）：系统的开机次数（Settings.Global.BOOT_COUNT，安卓 7 起，不要权限）。
     * 1.1 版用"当前时间 − 开机后经过的时间"算开机时刻，开机后联网校时一跳就当成新的一次开机，KOReader 退出后又被打开（2026-09-29 掌阅实测）。
     */
    private String bootId() {
        try {
            return "count:" + Settings.Global.getInt(getContentResolver(), Settings.Global.BOOT_COUNT);
        } catch (Settings.SettingNotFoundException | RuntimeException e) {
            return null;
        }
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
