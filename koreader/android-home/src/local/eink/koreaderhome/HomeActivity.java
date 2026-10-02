package local.eink.koreaderhome;

import android.app.Activity;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.os.Bundle;
import android.widget.Toast;

import java.util.List;

/**
 * 桌面（HOME）入口：系统要"回桌面"时（开机、在 KOReader 里退出）一律打开 KOReader——KOReader 就是桌面，退几次都回 KOReader（用户 2026-09-29）。
 * 只有找不到 KOReader（被卸载）时才打开别的桌面（掌阅原来的），不然会卡在空白。
 * 出口（掌阅没有主页键）：从屏幕顶端下拉系统控制中心 → 设置 → 默认应用 → 桌面改回「iReader 桌面」。自己不显示任何界面。
 */
public class HomeActivity extends Activity {
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
        if (!openKOReader()) {
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
