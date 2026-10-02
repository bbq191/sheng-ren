package local.eink.settingsprobe;

import android.app.Activity;
import android.content.ComponentName;
import android.content.Intent;
import android.content.pm.ActivityInfo;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.media.MediaScannerConnection;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.UserManager;
import android.provider.Settings;
import android.view.View;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

import java.io.BufferedReader;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.text.SimpleDateFormat;
import java.util.ArrayList;
import java.util.Collections;
import java.util.Date;
import java.util.List;
import java.util.Locale;
import java.util.regex.Pattern;

/**
 * 掌阅 Ocean 5 Pro 的设置里没有"关于本机 → 版本号"，也就开不了开发者选项和 USB 调试（2026-09-29 用户确认）。
 * 这里直接用 Intent 打开那些页面：掌阅要是只藏了入口、页面还在，就能进去开 USB 调试；能开 adb 才停得了系统应用，KOReader 独占才有意义。
 * 每个按钮点了打不开就在下面记一行。打开时把设备信息、设置应用里所有页面、全部已装应用写进 report.txt（Android/data/<包名>/files/，
 * USB 连电脑能读），点按钮的结果追加进去。自己不联网，不改任何设置。
 */
public class ProbeActivity extends Activity {
    /** 设置应用里名字带这些词的页面单独列成按钮。 */
    private static final Pattern INTERESTING = Pattern.compile(
            "(?i)develop|deviceinfo|aboutphone|aboutdevice|mydevice|about|build|search|debug|usb|adb|wireless");

    private TextView log;
    private TextView status;
    private File report;
    private String settingsPkg;
    private String homePkg;
    private String lastFlags;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        LinearLayout box = new LinearLayout(this);
        box.setOrientation(LinearLayout.VERTICAL);
        int pad = dp(16);
        box.setPadding(pad, pad, pad, pad);
        ScrollView scroll = new ScrollView(this);
        scroll.addView(box);
        setContentView(scroll);

        settingsPkg = settingsPackage();
        homePkg = otherHomePackage();

        status = text(box, status(settingsPkg, homePkg));
        status.setTextSize(15);

        header(box, "常规入口");
        button(box, "开发者选项", new Intent(Settings.ACTION_APPLICATION_DEVELOPMENT_SETTINGS));
        button(box, "关于本机（找到版本号连点 7 次）", new Intent(Settings.ACTION_DEVICE_INFO_SETTINGS));
        button(box, "设置搜索", new Intent("com.android.settings.action.SETTINGS_SEARCH"));
        button(box, "设置搜索（SettingsIntelligence）", component("com.android.settings.intelligence",
                "com.android.settings.intelligence.search.SearchActivity"));
        if (homePkg != null) {
            // 社区（Ocean 2）的做法：掌阅桌面崩溃后弹出的"应用信息"页右上角有个看不见的搜索按钮
            button(box, "掌阅桌面的应用信息（右上角可能有看不见的搜索）", new Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                    Uri.fromParts("package", homePkg, null)));
        }
        button(box, "设置首页", new Intent(Settings.ACTION_SETTINGS));

        List<ActivityInfo> pages = settingsActivities(settingsPkg);
        header(box, "设置应用（" + settingsPkg + "）里名字相关的页面");
        int n = 0;
        for (ActivityInfo a : pages) {
            if (a.exported && INTERESTING.matcher(a.name).find()) {
                button(box, shortName(a) + (a.enabled ? "" : "（已停用）"), component(a.packageName, a.name));
                n++;
            }
        }
        if (n == 0) {
            text(box, "（没有：设置应用的页面列表读不出来，或者没有名字相关的页面）");
        }

        header(box, "结果");
        log = text(box, "");
        writeReport(status.getText().toString(), settingsPkg, pages);
        lastFlags = flags();
    }

    /** 从系统设置页回来时刷新状态；开发者选项、USB 调试有变化就记进报告。 */
    @Override
    protected void onResume() {
        super.onResume();
        if (status == null) {
            return;
        }
        status.setText(status(settingsPkg, homePkg));
        String now = flags();
        if (lastFlags != null && !now.equals(lastFlags)) {
            String line = "状态变了：" + now;
            log.append(line + "\n");
            append(stamp() + " " + line + "\n");
        }
        lastFlags = now;
    }

    private String flags() {
        return "开发者选项开关 " + global(Settings.Global.DEVELOPMENT_SETTINGS_ENABLED)
                + "，USB 调试 " + global(Settings.Global.ADB_ENABLED);
    }

    // ── 打开页面 ──

    private void open(String label, Intent i) {
        i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        String result;
        try {
            startActivity(i);
            result = "✓ 打开了：" + label;
        } catch (RuntimeException e) {
            result = "✗ 打不开：" + label + "（" + e.getClass().getSimpleName() + ": " + e.getMessage() + "）";
        }
        log.append(result + "\n");
        append(stamp() + " " + result + "  " + i + "\n");
    }

    private static Intent component(String pkg, String cls) {
        return new Intent().setComponent(new ComponentName(pkg, cls));
    }

    // ── 查设备 ──

    /** 处理"打开设置"的应用：掌阅可能换成了自己的设置应用。 */
    private String settingsPackage() {
        ResolveInfo r = getPackageManager().resolveActivity(new Intent(Settings.ACTION_SETTINGS), PackageManager.MATCH_DEFAULT_ONLY);
        return r != null && r.activityInfo != null ? r.activityInfo.packageName : "com.android.settings";
    }

    /** 掌阅自己的桌面：系统里的桌面，跳过我们的「KOReader 桌面」（2026-10-02 真机：它还是默认桌面时，按钮打开的是它的信息页）。 */
    private String otherHomePackage() {
        Intent home = new Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_HOME);
        for (ResolveInfo r : getPackageManager().queryIntentActivities(home, 0)) {
            String pkg = r.activityInfo.packageName;
            if (!pkg.startsWith("local.eink.") && !"android".equals(pkg)) {
                return pkg;
            }
        }
        return null;
    }

    private List<ActivityInfo> settingsActivities(String pkg) {
        List<ActivityInfo> out = new ArrayList<>();
        try {
            PackageInfo pi = getPackageManager().getPackageInfo(pkg,
                    PackageManager.GET_ACTIVITIES | PackageManager.MATCH_DISABLED_COMPONENTS);
            if (pi.activities != null) {
                Collections.addAll(out, pi.activities);
            }
        } catch (PackageManager.NameNotFoundException e) {
            // 留空：按钮区会说读不出来
        }
        out.sort((a, b) -> a.name.compareTo(b.name));
        return out;
    }

    private String status(String settingsPkg, String homePkg) {
        return "型号 " + Build.MANUFACTURER + " " + Build.MODEL + "（" + Build.DEVICE + "）\n"
                + "安卓 " + Build.VERSION.RELEASE + "（API " + Build.VERSION.SDK_INT + "），版本 " + Build.DISPLAY + "\n"
                + "构建类型 " + Build.TYPE + " / " + Build.TAGS + "\n"
                + flags() + "\n"
                + "设置应用 " + settingsPkg + "，掌阅桌面 " + homePkg + "\n"
                + "报告 " + reportFile();
    }

    private String global(String key) {
        try {
            int v = Settings.Global.getInt(getContentResolver(), key);
            return v == 1 ? "开" : v == 0 ? "关" : String.valueOf(v);
        } catch (Settings.SettingNotFoundException e) {
            return "（没有这一项）";
        }
    }

    // ── 报告 ──

    private File reportFile() {
        if (report == null) {
            File dir = getExternalFilesDir(null);
            report = new File(dir != null ? dir : getFilesDir(), "report.txt");
        }
        return report;
    }

    private void writeReport(String status, String settingsPkg, List<ActivityInfo> pages) {
        StringBuilder sb = new StringBuilder();
        sb.append("# 设置入口 ").append(stamp()).append("\n").append(status).append("\n\n");
        sb.append("## 设置应用 ").append(settingsPkg).append(" 的全部页面（exported/enabled）\n");
        for (ActivityInfo a : pages) {
            sb.append(a.exported ? "E" : "-").append(a.enabled ? "+" : "x").append(" ").append(a.name).append("\n");
        }
        // 系统对当前用户设的限制：有 no_debugging_features 就是掌阅从系统层禁了调试，点版本号也开不了（2026-10-02 摸底）
        sb.append("\n## 用户限制（UserManager）\n");
        Bundle r = ((UserManager) getSystemService(USER_SERVICE)).getUserRestrictions();
        for (String k : new java.util.TreeSet<>(r.keySet())) {
            sb.append(k).append(" = ").append(r.get(k)).append("\n");
        }
        if (r.isEmpty()) {
            sb.append("（没有）\n");
        }
        // 系统属性：ro.debuggable、ro.adb.secure、USB 配置，以及掌阅自己加的（看有没有调试相关的开关）
        sb.append("\n## getprop\n").append(getprop());
        sb.append("\n## 已装应用（S = 系统应用，x = 已停用）\n");
        PackageManager pm = getPackageManager();
        List<ApplicationInfo> apps = pm.getInstalledApplications(PackageManager.MATCH_DISABLED_COMPONENTS);
        apps.sort((a, b) -> a.packageName.compareTo(b.packageName));
        for (ApplicationInfo a : apps) {
            sb.append((a.flags & ApplicationInfo.FLAG_SYSTEM) != 0 ? "S" : "-").append(a.enabled ? " " : "x")
                    .append(" ").append(a.packageName).append("  ").append(pm.getApplicationLabel(a)).append("\n");
        }
        sb.append("\n## 点按钮的结果\n");
        write(sb.toString(), false);
    }

    private static String getprop() {
        StringBuilder sb = new StringBuilder();
        try {
            Process p = new ProcessBuilder("getprop").redirectErrorStream(true).start();
            try (BufferedReader in = new BufferedReader(new InputStreamReader(p.getInputStream(), StandardCharsets.UTF_8))) {
                String line;
                while ((line = in.readLine()) != null) {
                    sb.append(line).append("\n");
                }
            }
        } catch (IOException e) {
            sb.append("（getprop 运行失败：").append(e.getMessage()).append("）\n");
        }
        return sb.toString();
    }

    private void append(String s) {
        write(s, true);
    }

    private void write(String s, boolean append) {
        File f = reportFile();
        try (FileOutputStream out = new FileOutputStream(f, append)) {
            out.write(s.getBytes(StandardCharsets.UTF_8));
        } catch (IOException e) {
            if (log != null) {
                log.append("（写报告失败：" + e.getMessage() + "）\n");
            }
            return;
        }
        // 让媒体库知道这个文件，USB（MTP）上才看得见
        MediaScannerConnection.scanFile(this, new String[]{f.getAbsolutePath()}, null, null);
    }

    private static String stamp() {
        return new SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.ROOT).format(new Date());
    }

    // ── 界面 ──

    private static String shortName(ActivityInfo a) {
        String n = a.name;
        if (n.startsWith(a.packageName + ".")) {
            n = n.substring(a.packageName.length());
        }
        return n;
    }

    private TextView text(LinearLayout box, String s) {
        TextView t = new TextView(this);
        t.setText(s);
        t.setTextSize(16);
        t.setTextColor(0xff000000);
        t.setTextIsSelectable(true);
        box.addView(t);
        return t;
    }

    private void header(LinearLayout box, String s) {
        TextView t = text(box, s);
        t.setTextSize(19);
        t.setPadding(0, dp(20), 0, dp(6));
    }

    private void button(LinearLayout box, String label, Intent i) {
        Button b = new Button(this);
        b.setText(label);
        b.setAllCaps(false);
        b.setTextSize(16);
        b.setOnClickListener((View v) -> open(label, new Intent(i)));
        box.addView(b);
    }

    private int dp(int v) {
        return Math.round(v * getResources().getDisplayMetrics().density);
    }
}
