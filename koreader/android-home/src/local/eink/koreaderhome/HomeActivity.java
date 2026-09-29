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
 * 桌面（HOME）入口：系统要"回桌面"时（开机、在 KOReader 里退出）打开 KOReader——KOReader 就是桌面（用户 2026-09-29）。
 * 出口：KOReader 被这里打开后 10 秒内又回到桌面（刚重开就又退出一次）→ 打开别的桌面（掌阅原来的）。
 * 所以"连着退出两次"就能真正退出 KOReader（下发配置前要这样），KOReader 一启动就崩也不会死循环。
 * 掌阅没有主页键，另一个出口是从屏幕顶端下拉系统控制中心 → 设置。找不到 KOReader 时也打开别的桌面。自己不显示任何界面。
 */
public class HomeActivity extends Activity {
    private static final String[] KOREADER = {"org.koreader.launcher", "org.koreader.launcher.fdroid"};
    private static final String PREFS = "home";
    /** 上次由这里打开 KOReader 的时刻（开机以来的毫秒）。 */
    private static final String KEY_LAST_OPEN = "koreader_opened_at";
    private static final long QUICK_EXIT_MS = 10_000;

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
        SharedPreferences prefs = getSharedPreferences(PREFS, MODE_PRIVATE);
        long now = SystemClock.elapsedRealtime();
        long last = prefs.getLong(KEY_LAST_OPEN, -1);
        // last > now：上次记的是上一次开机的时刻（开机后计时从 0 起），不算
        boolean quickExit = last >= 0 && last <= now && now - last < QUICK_EXIT_MS;
        if (quickExit) {
            prefs.edit().putLong(KEY_LAST_OPEN, -1).commit();
            openOtherHome();
        } else {
            prefs.edit().putLong(KEY_LAST_OPEN, now).commit();
            if (!openKOReader()) {
                openOtherHome();
            }
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
